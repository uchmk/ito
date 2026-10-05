//! Which wgpu backends to draw with (the `wgpu` feature).
//!
//! Windows draws with GL first when the machine has it: under Vulkan and DX12
//! an AMD driver thread spins a core for a window that is doing nothing
//! (filer #232, #240), and GL drew both of filer's test machines, the ARM64
//! one through a translation layer (#239). Elsewhere wgpu chooses.

use egui_wgpu::wgpu::{self, Backends};

/// The backends to narrow wgpu to for a backend the user named (`None` for
/// `auto`): `Ok` with what to use (`None` leaves wgpu its own pick), or `Err`
/// with `auto`'s choice when this machine has no adapter for the named one --
/// the app says so in its own words. Falling back to `auto` rather than to
/// wgpu's pick matters: a typo put the spinning core back (filer #243, #244).
pub fn pick_backends(
    name: Option<&str>,
    windows: bool,
    has: impl Fn(Backends) -> bool,
) -> Result<Option<Backends>, Option<Backends>> {
    let auto = || auto_backends(windows, || has(Backends::GL));
    let Some(name) = name else { return Ok(auto()) };
    let backends = Backends::from_comma_list(name);
    match has(backends) {
        true => Ok(Some(backends)),
        false => Err(auto()),
    }
}

/// What `auto` narrows the backends to: GL on Windows when this machine has
/// it, else nothing, which leaves wgpu to choose. `gl_ok` is only asked on
/// Windows, since the question costs an instance of its own.
pub fn auto_backends(windows: bool, gl_ok: impl FnOnce() -> bool) -> Option<Backends> {
    (windows && gl_ok()).then_some(Backends::GL)
}

/// Whether wgpu finds an adapter on `backends`. Asked of an instance of its
/// own, before the window exists; the answer is ready at once on every native
/// backend, and one still pending is taken as a yes rather than waited on.
pub fn has_adapter(backends: Backends) -> bool {
    use std::future::Future as _;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut probe = std::pin::pin!(instance.enumerate_adapters(backends));
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match probe.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(adapters) => !adapters.is_empty(),
        std::task::Poll::Pending => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend with no adapter falls back to `auto`, GL on Windows -- not
    /// wgpu's pick, which spun the core again (filer #243).
    #[test]
    fn a_missing_backend_falls_back_to_auto() {
        let gl_only = |b: Backends| b == Backends::GL;
        assert_eq!(pick_backends(Some("vulkan"), true, gl_only), Err(Some(Backends::GL)));
        assert_eq!(pick_backends(Some("vulkan"), true, |_| true), Ok(Some(Backends::VULKAN)));
        assert_eq!(pick_backends(None, true, gl_only), Ok(Some(Backends::GL)));
        assert_eq!(pick_backends(None, false, |_| true), Ok(None), "auto off Windows: wgpu's pick");
    }

    /// `auto` is GL on Windows when it is there, wgpu's pick otherwise, and
    /// does not even ask about GL elsewhere (filer, 2026-10-04, over Q70).
    #[test]
    fn auto_is_gl_on_windows_only() {
        assert_eq!(auto_backends(true, || true), Some(Backends::GL));
        assert_eq!(auto_backends(true, || false), None, "no GL: wgpu's own pick");
        assert_eq!(auto_backends(false, || panic!("asked about GL off Windows")), None);
    }
}
