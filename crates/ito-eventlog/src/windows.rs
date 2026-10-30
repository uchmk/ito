//! Windows: `RegisterEventSourceW` / `ReportEventW` to the Application log; `EvtQuery` to read a log.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::EventLog::{
    DeregisterEventSource, EvtClose, EvtFormatMessage, EvtFormatMessageEvent, EvtNext, EvtOpenPublisherMetadata, EvtQuery,
    EvtQueryChannelPath, EvtQueryForwardDirection, EvtQueryReverseDirection, EvtRender, EvtRenderEventXml, RegisterEventSourceW,
    ReportEventW, EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE, EVENTLOG_WARNING_TYPE, EVT_HANDLE,
};

use crate::{Filter, From, Level, Record, WhyNot};

/// Most records one `poll` returns; the rest comes on the next call.
const PER_POLL: usize = 1000;
const ACCESS_DENIED: u32 = 0x8007_0005;
const NO_CHANNEL: u32 = 0x8007_3A9F;
const NO_MORE_ITEMS: u32 = 0x8007_0103;
const INVALID_PARAMETER: u32 = 0x8007_0057;

#[derive(Debug)]
pub struct Source(HANDLE);

// SAFETY: an event source handle is a kernel object handle, usable from any thread.
unsafe impl Send for Source {}
unsafe impl Sync for Source {}

impl Drop for Source {
    fn drop(&mut self) {
        // SAFETY: the handle came from RegisterEventSourceW and is closed once.
        unsafe {
            let _ = DeregisterEventSource(self.0);
        }
    }
}

fn failed(e: windows::core::Error) -> WhyNot {
    match e.code().0 as u32 {
        ACCESS_DENIED => WhyNot::Denied,
        NO_CHANNEL => WhyNot::NoChannel,
        n => WhyNot::Failed(n),
    }
}

pub fn open(source: &str) -> Result<Source, WhyNot> {
    let name = HSTRING::from(source);
    // SAFETY: `name` outlives the call; the handle is closed in Drop.
    unsafe { RegisterEventSourceW(PCWSTR::null(), &name) }.map(Source).map_err(failed)
}

impl Source {
    pub fn write(&self, level: Level, id: u32, text: &str) -> Result<(), WhyNot> {
        let kind = match level {
            Level::Info => EVENTLOG_INFORMATION_TYPE,
            Level::Warning => EVENTLOG_WARNING_TYPE,
            Level::Error => EVENTLOG_ERROR_TYPE,
        };
        let text = HSTRING::from(text);
        let strings = [PCWSTR(text.as_ptr())];
        // SAFETY: `strings` points at `text`, which outlives the call.
        unsafe { ReportEventW(self.0, kind, 0, id, None, 0, Some(&strings), None) }.map_err(failed)
    }
}

/// An event-log handle, closed on drop.
struct Handle(EVT_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from an Evt* call and is closed once.
        unsafe {
            let _ = EvtClose(self.0);
        }
    }
}

#[derive(Debug)]
pub struct Reader {
    channel: String,
    filter: Filter,
    last: u64,
}

pub fn reader(channel: &str, filter: &Filter, from: From) -> Result<Reader, WhyNot> {
    // Ask for the newest record once: a missing log or a refusal shows now, not at the first poll.
    let newest = newest(channel)?;
    let last = match from {
        From::End => newest,
        From::Start => 0,
        From::After(n) => n,
    };
    let r = Reader { channel: channel.to_owned(), filter: filter.clone(), last };
    r.xpath()?;
    Ok(r)
}

fn newest(channel: &str) -> Result<u64, WhyNot> {
    let q = query(channel, "*", EvtQueryReverseDirection.0)?;
    Ok(next(&q, 1)?.first().and_then(|h| render(h).ok()).and_then(|x| text(&x, "EventRecordID")?.parse().ok()).unwrap_or(0))
}

fn query(channel: &str, xpath: &str, direction: u32) -> Result<Handle, WhyNot> {
    let (c, x) = (HSTRING::from(channel), HSTRING::from(xpath));
    // SAFETY: both strings outlive the call; the handle is closed in Drop.
    unsafe { EvtQuery(None, &c, &x, EvtQueryChannelPath.0 | direction) }.map(Handle).map_err(failed)
}

/// Up to `n` events of the result set (none left is an empty list).
fn next(q: &Handle, n: usize) -> Result<Vec<Handle>, WhyNot> {
    let mut raw = vec![0isize; n];
    let mut got = 0u32;
    // SAFETY: `raw` holds `n` slots and `got` receives how many were filled.
    match unsafe { EvtNext(q.0, &mut raw, 0, 0, &mut got) } {
        Ok(()) => Ok(raw[..got as usize].iter().map(|&h| Handle(EVT_HANDLE(h))).collect()),
        Err(e) if e.code().0 as u32 == NO_MORE_ITEMS => Ok(Vec::new()),
        Err(e) => Err(failed(e)),
    }
}

fn render(event: &Handle) -> Result<String, WhyNot> {
    let (mut used, mut props) = (0u32, 0u32);
    // SAFETY: no buffer: only asks how many bytes are needed.
    let _ = unsafe { EvtRender(None, event.0, EvtRenderEventXml.0, 0, None, &mut used, &mut props) };
    let mut buf = vec![0u16; used as usize / 2 + 1];
    // SAFETY: `buf` is at least `used` bytes long.
    unsafe { EvtRender(None, event.0, EvtRenderEventXml.0, (buf.len() * 2) as u32, Some(buf.as_mut_ptr().cast()), &mut used, &mut props) }
        .map_err(failed)?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(String::from_utf16_lossy(&buf[..len]))
}

/// The text the provider registered for this event, when this PC has the provider's message file.
fn format(meta: &Handle, event: &Handle) -> Option<String> {
    let mut used = 0u32;
    // SAFETY: no buffer: asks for the size in UTF-16 units.
    let _ = unsafe { EvtFormatMessage(Some(meta.0), Some(event.0), 0, None, EvtFormatMessageEvent.0, None, &mut used) };
    if used == 0 {
        return None;
    }
    let mut buf = vec![0u16; used as usize];
    // SAFETY: `buf` has `used` units.
    unsafe { EvtFormatMessage(Some(meta.0), Some(event.0), 0, None, EvtFormatMessageEvent.0, Some(&mut buf), &mut used) }.ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..len]).trim().to_owned();
    (!s.is_empty()).then_some(s)
}

fn quote(v: &str) -> Result<String, WhyNot> {
    match (v.contains('\''), v.contains('"')) {
        (false, _) => Ok(format!("'{v}'")),
        (true, false) => Ok(format!("\"{v}\"")),
        (true, true) => Err(WhyNot::Failed(INVALID_PARAMETER)),
    }
}

impl Reader {
    fn xpath(&self) -> Result<String, WhyNot> {
        let mut system = format!("(EventRecordID > {})", self.last);
        if !self.filter.ids.is_empty() {
            let ids: Vec<String> = self.filter.ids.iter().map(|i| format!("EventID={}", i & 0xFFFF)).collect();
            system.push_str(&format!(" and ({})", ids.join(" or ")));
        }
        if let Some(s) = &self.filter.source {
            system.push_str(&format!(" and Provider[@Name={}]", quote(s)?));
        }
        match self.filter.level {
            Some(Level::Error) => system.push_str(" and (Level=1 or Level=2)"),
            Some(Level::Warning) => system.push_str(" and Level=3"),
            Some(Level::Info) => system.push_str(" and (Level=0 or Level=4 or Level=5)"),
            None => {}
        }
        Ok(format!("*[System[{system}]]"))
    }

    pub fn poll(&mut self) -> Result<Vec<Record>, WhyNot> {
        let q = query(&self.channel, &self.xpath()?, EvtQueryForwardDirection.0)?;
        let mut out = Vec::new();
        let mut metas: Vec<(String, Option<Handle>)> = Vec::new();
        while out.len() < PER_POLL {
            let batch = next(&q, 64.min(PER_POLL - out.len()))?;
            if batch.is_empty() {
                break;
            }
            for event in &batch {
                let Ok(xml) = render(event) else { continue };
                let Some(mut rec) = parse(&xml) else { continue };
                if !metas.iter().any(|(n, _)| *n == rec.source) {
                    let name = HSTRING::from(rec.source.as_str());
                    // SAFETY: `name` outlives the call; the handle is closed in Drop.
                    let meta = unsafe { EvtOpenPublisherMetadata(None, &name, PCWSTR::null(), 0, 0) }.ok().map(Handle);
                    metas.push((rec.source.clone(), meta));
                }
                let meta = metas.iter().find(|(n, _)| *n == rec.source).and_then(|(_, m)| m.as_ref());
                if let Some(text) = meta.and_then(|m| format(m, event)) {
                    rec.message = text;
                }
                out.push(rec);
            }
        }
        if let Some(r) = out.last() {
            self.last = r.record_id;
        }
        Ok(out)
    }

    pub fn position(&self) -> u64 {
        self.last
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The text of the first `<tag>…</tag>` (or `<tag attr…>…</tag>`).
fn text(xml: &str, tag: &str) -> Option<String> {
    let at = xml.find(&format!("<{tag}>")).map(|i| i + tag.len() + 2).or_else(|| {
        let i = xml.find(&format!("<{tag} "))?;
        Some(i + xml[i..].find('>')? + 1)
    })?;
    let end = xml[at..].find(&format!("</{tag}>"))?;
    Some(unescape(&xml[at..at + end]))
}

/// The value of `name` on the first `<tag …>`.
fn attr(xml: &str, tag: &str, name: &str) -> Option<String> {
    let i = xml.find(&format!("<{tag} "))?;
    let el = &xml[i..i + xml[i..].find('>')?];
    for q in ['\'', '"'] {
        if let Some(a) = el.find(&format!("{name}={q}")) {
            let from = a + name.len() + 2;
            let to = el[from..].find(q)?;
            return Some(unescape(&el[from..from + to]));
        }
    }
    None
}

fn parse(xml: &str) -> Option<Record> {
    let level = match text(xml, "Level").and_then(|l| l.parse::<u32>().ok()) {
        Some(1 | 2) => Level::Error,
        Some(3) => Level::Warning,
        _ => Level::Info,
    };
    let mut data = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<Data") {
        rest = &rest[i + 5..];
        let Some(open) = rest.find('>') else { break };
        if rest[..open].ends_with('/') {
            continue;
        }
        if let Some(close) = rest.find("</Data>") {
            data.push(unescape(&rest[open + 1..close]));
        }
    }
    Some(Record {
        record_id: text(xml, "EventRecordID")?.parse().ok()?,
        time: attr(xml, "TimeCreated", "SystemTime").unwrap_or_default(),
        id: text(xml, "EventID")?.parse().ok()?,
        source: attr(xml, "Provider", "Name").unwrap_or_default(),
        level,
        message: data.join(" "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_xml_is_read() {
        let xml = "<Event xmlns='x'><System><Provider Name='My &amp; App'/><EventID Qualifiers='0'>77</EventID><Level>3</Level>\
            <TimeCreated SystemTime='2026-10-31T01:02:03.1234567Z'/><EventRecordID>9001</EventRecordID></System>\
            <EventData><Data>one</Data><Data Name='b'>t&lt;wo</Data><Data/></EventData></Event>";
        let r = parse(xml).unwrap();
        assert_eq!((r.record_id, r.id, r.level), (9001, 77, Level::Warning));
        assert_eq!((r.source.as_str(), r.time.as_str(), r.message.as_str()), ("My & App", "2026-10-31T01:02:03.1234567Z", "one t<wo"));
    }

    #[test]
    fn the_query_is_built_from_the_filter() {
        let f = Filter { ids: vec![1, 0x10002], source: Some("O'x".into()), level: Some(Level::Error) };
        let r = Reader { channel: "Application".into(), filter: f, last: 5 };
        assert_eq!(
            r.xpath().unwrap(),
            "*[System[(EventRecordID > 5) and (EventID=1 or EventID=2) and Provider[@Name=\"O'x\"] and (Level=1 or Level=2)]]"
        );
        let both = Filter { source: Some("a'\"b".into()), ..Filter::default() };
        assert!(Reader { channel: String::new(), filter: both, last: 0 }.xpath().is_err());
    }
}
