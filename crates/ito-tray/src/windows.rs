//! Windows: a thread with a hidden window that owns the icon (`Shell_NotifyIconW`), shows the menu and
//! queues the events. The API side only changes the shared state and posts a message to that window.

use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HGDIOBJ};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, GetWindowLongPtrW, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, TranslateMessage, CREATESTRUCTW, GWLP_USERDATA, HICON, HMENU, ICONINFO,
    MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_DESTROY, WM_LBUTTONUP,
    WM_NCCREATE, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_POPUP,
};

use crate::{tip_text, with_badge, Event, Icon, Item, State};

/// From the tray icon: the low word of `lparam` is the mouse message.
const WM_TRAY: u32 = WM_APP + 1;
/// From the API: the shared state changed, redraw the icon.
const WM_UPDATE: u32 = WM_APP + 2;
const WM_CLOSE: u32 = 0x0010;

struct Shared {
    icon: Icon,
    state: Option<State>,
    tip: String,
    menu: Vec<Item>,
}

struct Ctx {
    shared: Arc<Mutex<Shared>>,
    tx: Sender<Event>,
    wake: Box<dyn Fn() + Send>,
    hicon: Option<HICON>,
    taskbar_created: u32,
}

// SAFETY: the icon handle is made and used only on the tray thread (it is `None` while `Ctx` moves there).
unsafe impl Send for Ctx {}

pub struct Tray {
    shared: Arc<Mutex<Shared>>,
    hwnd: isize,
    rx: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    pub fn new(icon: Icon, tip: &str, wake: Box<dyn Fn() + Send>) -> io::Result<Tray> {
        let shared = Arc::new(Mutex::new(Shared { icon, state: None, tip: tip.to_owned(), menu: Vec::new() }));
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<io::Result<isize>>(1);
        let ctx = Ctx { shared: shared.clone(), tx, wake, hicon: None, taskbar_created: 0 };
        let thread = std::thread::Builder::new().name("ito-tray".into()).spawn(move || run(ctx, ready_tx))?;
        let hwnd = ready_rx.recv().map_err(|_| io::Error::other("the tray thread ended"))??;
        Ok(Tray { shared, hwnd, rx, thread: Some(thread) })
    }

    fn change(&self, f: impl FnOnce(&mut Shared)) {
        f(&mut self.shared.lock().unwrap());
        // SAFETY: posting to a window of ours; a failure means it is gone.
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_UPDATE, WPARAM(0), LPARAM(0));
        }
    }

    pub fn set_state(&self, state: Option<State>) {
        self.change(|s| s.state = state);
    }

    pub fn set_tip(&self, tip: &str) {
        self.change(|s| s.tip = tip.to_owned());
    }

    pub fn set_menu(&self, items: Vec<Item>) {
        self.shared.lock().unwrap().menu = items;
    }

    pub fn events(&self) -> &Receiver<Event> {
        &self.rx
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: as in `change`.
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(ctx: Ctx, ready: mpsc::SyncSender<io::Result<isize>>) {
    // SAFETY: plain window-class and window creation; `ctx` is leaked to the window procedure and taken back
    // after the loop ends.
    unsafe {
        let hinst = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default();
        let class = w!("ito-tray");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: hinst, lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc); // 0 when it is registered already, which is fine
        let ptr = Box::into_raw(Box::new(ctx));
        (*ptr).taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        // A hidden top-level window (a message-only one gets no `TaskbarCreated`).
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), Some(ptr.cast()));
        let hwnd = match hwnd {
            Ok(h) => h,
            Err(e) => {
                drop(Box::from_raw(ptr));
                let _ = ready.send(Err(io::Error::other(e)));
                return;
            }
        };
        if !notify(hwnd, &mut *ptr, NIM_ADD) {
            let _ = ready.send(Err(io::Error::other("the shell did not take the tray icon")));
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
            drop(Box::from_raw(ptr));
            return;
        }
        let _ = ready.send(Ok(hwnd.0 as isize));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        drop(Box::from_raw(ptr));
    }
}

/// Add, change or delete the icon. `false` if the shell refused.
unsafe fn notify(hwnd: HWND, ctx: &mut Ctx, op: windows::Win32::UI::Shell::NOTIFY_ICON_MESSAGE) -> bool {
    let (picture, tip) = {
        let s = ctx.shared.lock().unwrap();
        let picture = match s.state {
            Some(st) => with_badge(&s.icon, st),
            None => s.icon.clone(),
        };
        (picture, tip_text(s.state, &s.tip))
    };
    if let Some(old) = ctx.hicon.take() {
        let _ = DestroyIcon(old);
    }
    ctx.hicon = make_icon(&picture);
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: ctx.hicon.unwrap_or_default(),
        ..Default::default()
    };
    for (i, c) in tip.encode_utf16().take(127).enumerate() {
        nid.szTip[i] = c;
    }
    Shell_NotifyIconW(op, &nid).as_bool()
}

/// An HICON from RGBA pixels (BGRA for the system; the alpha channel is the shape).
unsafe fn make_icon(icon: &Icon) -> Option<HICON> {
    let mut bgra = icon.rgba.clone();
    for p in bgra.chunks_mut(4) {
        p.swap(0, 2);
    }
    let color = CreateBitmap(icon.width as i32, icon.height as i32, 1, 32, Some(bgra.as_ptr().cast()));
    let mask = CreateBitmap(icon.width as i32, icon.height as i32, 1, 1, None);
    let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
    let made = CreateIconIndirect(&info).ok();
    let _ = DeleteObject(HGDIOBJ(color.0));
    let _ = DeleteObject(HGDIOBJ(mask.0));
    made
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
    if ctx.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let ctx = &mut *ctx;
    match msg {
        WM_TRAY => {
            match (lparam.0 as u32) & 0xFFFF {
                WM_LBUTTONUP => send(ctx, Event::Click),
                WM_RBUTTONUP => {
                    if let Some(id) = pick(hwnd, ctx) {
                        send(ctx, Event::Menu(id));
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_UPDATE => {
            notify(hwnd, ctx, NIM_MODIFY);
            LRESULT(0)
        }
        WM_DESTROY => {
            notify(hwnd, ctx, NIM_DELETE);
            if let Some(h) = ctx.hicon.take() {
                let _ = DestroyIcon(h);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if m == ctx.taskbar_created && m != 0 => {
            // Explorer restarted: the icon is gone, put it back.
            notify(hwnd, ctx, NIM_ADD);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn send(ctx: &Ctx, e: Event) {
    let _ = ctx.tx.send(e);
    (ctx.wake)();
}

/// Show the menu at the cursor and return the id of the entry chosen.
unsafe fn pick(hwnd: HWND, ctx: &Ctx) -> Option<String> {
    let items = ctx.shared.lock().unwrap().menu.clone();
    if items.is_empty() {
        return None;
    }
    let menu: HMENU = CreatePopupMenu().ok()?;
    let mut ids = Vec::new();
    for item in &items {
        match item {
            Item::Separator => {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            Item::Entry { id, label, enabled, checked } => {
                ids.push(id.clone());
                let mut flags = MF_STRING;
                if !enabled {
                    flags |= MF_GRAYED;
                }
                if *checked {
                    flags |= MF_CHECKED;
                }
                // `&` marks a shortcut letter in a menu; a label has none.
                let text: Vec<u16> = label.replace('&', "&&").encode_utf16().chain(Some(0)).collect();
                let _ = AppendMenuW(menu, flags, ids.len(), PCWSTR(text.as_ptr()));
            }
        }
    }
    let mut at = Default::default();
    let _ = GetCursorPos(&mut at);
    // The menu closes on a click elsewhere only when its window is in front.
    let _ = SetForegroundWindow(hwnd);
    let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, Some(0), hwnd, None);
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    let n = chosen.0 as usize;
    (n >= 1).then(|| ids.get(n - 1).cloned()).flatten()
}
