//! A Model Context Protocol server over stdio, small enough to read: an LLM
//! client (Claude Code, Claude Desktop) starts the program, writes JSON-RPC
//! requests to its stdin one per line, and reads the answers from its stdout.
//! An app registers [`Tool`]s and calls [`Server::serve`]; this answers
//! `initialize`, `ping`, `tools/list` and `tools/call`, and leaves the rest.
//!
//! No async runtime: each request is answered before the next is read, which
//! is all a tool that asks a running window a question needs. The design is
//! filer's docs/llm-integration.md.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The JSON this speaks, for building schemas and answers without a second
/// dependency line.
pub use serde_json;

/// The protocol versions this speaks, newest first. The client names the one
/// it wants at `initialize`; one of these is the answer either way, as the
/// specification asks.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What a tool's call gives back: the text for the model, or an error the
/// model is told about (`isError`), such as "filer is not running".
pub type Answer = Result<String, String>;

/// One tool: its name, what it is for (the model reads this to decide when to
/// call it), the JSON Schema of its arguments, and the function.
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    /// Changes nothing (MCP's `readOnlyHint`). Clients may call such a tool
    /// without asking the person first.
    pub read_only: bool,
    /// Throws something away or closes it (MCP's `destructiveHint`).
    pub destructive: bool,
    /// The app puts a confirmation box in its window before it acts (sent as `_meta["ito/needsConfirm"]`,
    /// for a client that wants to say so); the box itself is the app's (`ito-confirm`).
    pub needs_confirm: bool,
    pub call: Box<dyn Fn(&Value) -> Answer + Send + Sync>,
}

impl Tool {
    /// A tool that takes no arguments.
    pub fn new(name: &str, description: &str, call: impl Fn(&Value) -> Answer + Send + Sync + 'static) -> Self {
        let input_schema = json!({ "type": "object", "properties": {} });
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            read_only: true,
            destructive: false,
            needs_confirm: false,
            call: Box::new(call),
        }
    }

    /// Its arguments, as a JSON Schema object.
    pub fn input(mut self, schema: Value) -> Self {
        self.input_schema = schema;
        self
    }

    /// Changes something (a file, the screen a person is looking at).
    pub fn writes(mut self) -> Self {
        self.read_only = false;
        self
    }

    /// Closes or deletes something (implies [`Tool::writes`]).
    pub fn destructive(mut self) -> Self {
        self.read_only = false;
        self.destructive = true;
        self
    }

    /// A person confirms in the app's window before this acts (implies [`Tool::writes`]).
    pub fn needs_confirm(mut self) -> Self {
        self.read_only = false;
        self.needs_confirm = true;
        self
    }
}

pub struct Server {
    name: String,
    version: String,
    instructions: Option<String>,
    tools: Vec<Tool>,
}

// JSON-RPC's own error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

impl Server {
    /// `name` and `version` are what the client shows for the server.
    pub fn new(name: &str, version: &str) -> Self {
        Self { name: name.into(), version: version.into(), instructions: None, tools: Vec::new() }
    }

    /// Words for the model about the server as a whole, sent at `initialize`.
    pub fn instructions(mut self, text: &str) -> Self {
        self.instructions = Some(text.into());
        self
    }

    pub fn tool(mut self, tool: Tool) -> Self {
        self.tools.push(tool);
        self
    }

    /// Answer requests from `input` on `output` until `input` ends (the
    /// client closed stdin, which is how it stops the server).
    pub fn serve(&self, input: impl BufRead, mut output: impl Write) -> io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(answer) = self.handle(&line) {
                output.write_all(answer.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    /// One message in, its answer out: `None` for a notification, which is
    /// not answered.
    pub fn handle(&self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, PARSE_ERROR, &format!("not JSON: {e}"))),
        };
        // A batch (an array) was in 2025-03-26 only, and is gone again.
        let Some(obj) = msg.as_object() else { return Some(error(Value::Null, INVALID_REQUEST, "one JSON-RPC object per line")) };
        let method = obj.get("method").and_then(Value::as_str);
        let Some(id) = obj.get("id").cloned() else {
            // A notification (`notifications/initialized`, a cancellation):
            // nothing to say back. A response to a request of ours cannot
            // happen, as this never sends one.
            return None;
        };
        let Some(method) = method else { return Some(error(id, INVALID_REQUEST, "no method")) };
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        Some(match self.call(method, &params) {
            Ok(result) => serde_json::to_string(&json!({ "jsonrpc": "2.0", "id": id, "result": result })).unwrap_or_default(),
            Err((code, message)) => error(id, code, &message),
        })
    }

    fn call(&self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let version = asked.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
                let mut result = json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": self.name, "version": self.version },
                });
                if let Some(text) = &self.instructions {
                    result["instructions"] = json!(text);
                }
                Ok(result)
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                let tools: Vec<Value> = self
                    .tools
                    .iter()
                    .map(|t| {
                        let mut tool = json!({
                            "name": t.name,
                            "description": t.description,
                            "inputSchema": t.input_schema,
                            "annotations": { "readOnlyHint": t.read_only },
                        });
                        if t.destructive {
                            tool["annotations"]["destructiveHint"] = json!(true);
                        }
                        if t.needs_confirm {
                            tool["_meta"] = json!({ "ito/needsConfirm": true });
                        }
                        tool
                    })
                    .collect();
                Ok(json!({ "tools": tools }))
            }
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "tools/call needs a name".to_string()))?;
                let tool = self.tools.iter().find(|t| t.name == name).ok_or_else(|| (INVALID_PARAMS, format!("unknown tool: {name}")))?;
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let (text, failed) = match (tool.call)(&args) {
                    Ok(text) => (text, false),
                    Err(text) => (text, true),
                };
                Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": failed }))
            }
            other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
        }
    }
}

fn error(id: Value, code: i64, message: &str) -> String {
    serde_json::to_string(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })).unwrap_or_default()
}

/// A string argument, or an error the model can act on.
pub fn arg_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name).and_then(Value::as_str).ok_or_else(|| format!("`{name}` is missing (a string)"))
}

/// What the agent opened through this server, so a tool that closes can close only those ("close only
/// what the agent opened"). Cheap to clone; every clone sees the same set.
#[derive(Clone, Debug, Default)]
pub struct Opened(Arc<Mutex<BTreeSet<String>>>);

impl Opened {
    pub fn new() -> Self {
        Self::default()
    }

    /// Note that the agent opened `id` (a pane, a tab, a file).
    pub fn insert(&self, id: &str) {
        self.set().insert(id.to_owned());
    }

    pub fn contains(&self, id: &str) -> bool {
        self.set().contains(id)
    }

    /// Forget `id` (it was closed, by the agent or by the person); whether it was there.
    pub fn remove(&self, id: &str) -> bool {
        self.set().remove(id)
    }

    /// `Ok` when the agent opened `id`, else the error to give back as the tool's answer.
    pub fn check(&self, id: &str) -> Answer {
        if self.contains(id) {
            Ok(id.to_owned())
        } else {
            Err(format!("`{id}` was not opened by the agent, so it is not closed from here"))
        }
    }

    pub fn ids(&self) -> Vec<String> {
        self.set().iter().cloned().collect()
    }

    fn set(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Where a supervised session is: the words agents and apps share, so `status` and `wait` mean the same
/// in every app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Running,
    /// Stopped for a person or another agent to answer.
    Waiting,
    Done,
    Error,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Running => "running",
            Phase::Waiting => "waiting",
            Phase::Done => "done",
            Phase::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Phase> {
        [Phase::Running, Phase::Waiting, Phase::Done, Phase::Error].into_iter().find(|p| p.as_str() == s)
    }

    /// Nothing more will happen without someone acting (done or failed).
    pub fn is_finished(self) -> bool {
        matches!(self, Phase::Done | Phase::Error)
    }
}

/// The longest one tool call waits. `ito-mcp` answers one request at a time, and clients give up on a
/// call after about a minute, so a longer wait is asked again.
pub const MAX_WAIT: Duration = Duration::from_secs(50);

/// Call `check` every `every` until it gives `Some`, or `timeout` (at most [`MAX_WAIT`]) passes: then `None`.
/// For a `wait` tool: `wait(timeout, every, || phase_of(session).filter(|p| *p != Phase::Running))`.
pub fn wait<T>(timeout: Duration, every: Duration, mut check: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + timeout.min(MAX_WAIT);
    loop {
        if let Some(v) = check() {
            return Some(v);
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        std::thread::sleep(every.min(left));
    }
}

/// An optional number-of-seconds argument as a [`Duration`] (`default` when missing).
pub fn arg_secs(args: &Value, name: &str, default: u64) -> Duration {
    Duration::from_secs(args.get(name).and_then(Value::as_u64).unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server::new("test", "1.2.3")
            .instructions("for tests")
            .tool(Tool::new("echo", "Says its word back.", |a| arg_str(a, "word").map(str::to_owned)).input(json!({
                "type": "object",
                "properties": { "word": { "type": "string" } },
                "required": ["word"],
            })))
            .tool(Tool::new("move", "Moves something.", |_| Ok("moved".into())).writes())
    }

    fn ask(s: &Server, line: &str) -> Value {
        serde_json::from_str(&s.handle(line).expect("an answer")).unwrap()
    }

    #[test]
    fn initialize_names_the_server_and_a_version_it_speaks() {
        let s = server();
        let a = ask(
            &s,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}"#,
        );
        assert_eq!(a["id"], 1);
        assert_eq!(a["result"]["protocolVersion"], "2025-06-18", "the one asked for, when known");
        assert_eq!(a["result"]["serverInfo"]["name"], "test");
        assert_eq!(a["result"]["serverInfo"]["version"], "1.2.3");
        assert!(a["result"]["capabilities"]["tools"].is_object());
        assert_eq!(a["result"]["instructions"], "for tests");
        let a = ask(&s, r#"{"jsonrpc":"2.0","id":"x","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#);
        assert_eq!(a["id"], "x", "a string id comes back as it went");
        assert_eq!(a["result"]["protocolVersion"], PROTOCOL_VERSIONS[0], "else the newest");
    }

    #[test]
    fn notifications_are_not_answered() {
        assert_eq!(server().handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
    }

    #[test]
    fn tools_are_listed_with_their_schema_and_whether_they_write() {
        let a = ask(&server(), r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let tools = a["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[0]["inputSchema"]["required"][0], "word");
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
        assert_eq!(tools[1]["inputSchema"]["type"], "object", "no arguments is still an object schema");
        assert_eq!(tools[1]["annotations"]["readOnlyHint"], false);
    }

    #[test]
    fn destructive_and_confirm_marks_are_listed() {
        let s = Server::new("t", "1")
            .tool(Tool::new("close", "Closes.", |_| Ok(String::new())).destructive())
            .tool(Tool::new("send", "Sends.", |_| Ok(String::new())).needs_confirm());
        let a = ask(&s, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let t = a["result"]["tools"].as_array().unwrap();
        assert_eq!(t[0]["annotations"]["readOnlyHint"], false);
        assert_eq!(t[0]["annotations"]["destructiveHint"], true);
        assert!(t[0]["_meta"].is_null());
        assert_eq!(t[1]["_meta"]["ito/needsConfirm"], true);
        assert!(t[1]["annotations"]["destructiveHint"].is_null());
    }

    #[test]
    fn only_what_the_agent_opened_passes_the_check() {
        let o = Opened::new();
        o.insert("pane-1");
        let shared = o.clone();
        assert_eq!(shared.check("pane-1"), Ok("pane-1".into()));
        assert!(shared.check("pane-2").unwrap_err().contains("not opened"));
        assert!(o.remove("pane-1") && !shared.contains("pane-1"));
    }

    #[test]
    fn wait_returns_when_it_changes_or_gives_up() {
        let mut n = 0;
        let got = wait(Duration::from_secs(5), Duration::from_millis(1), || {
            n += 1;
            (n == 3).then_some(Phase::Done)
        });
        assert_eq!(got, Some(Phase::Done));
        assert_eq!(wait(Duration::from_millis(20), Duration::from_millis(5), || None::<u8>), None);
        assert_eq!(Phase::parse("waiting"), Some(Phase::Waiting));
        assert!(Phase::Error.is_finished() && !Phase::Waiting.is_finished());
        assert_eq!(arg_secs(&json!({"t": 7}), "t", 1), Duration::from_secs(7));
        assert_eq!(arg_secs(&json!({}), "t", 1), Duration::from_secs(1));
    }

    #[test]
    fn a_call_gives_text_and_a_failed_call_says_so() {
        let s = server();
        let a = ask(&s, r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"echo","arguments":{"word":"hi"}}}"#);
        assert_eq!(a["result"]["content"][0]["type"], "text");
        assert_eq!(a["result"]["content"][0]["text"], "hi");
        assert_eq!(a["result"]["isError"], false);
        let a = ask(&s, r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"echo","arguments":{}}}"#);
        assert_eq!(a["result"]["isError"], true, "the tool's own failure is a result the model reads");
        assert!(a["result"]["content"][0]["text"].as_str().unwrap().contains("word"));
        let a = ask(&s, r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}"#);
        assert_eq!(a["error"]["code"], INVALID_PARAMS, "an unknown tool is a protocol error");
    }

    #[test]
    fn unknown_methods_and_broken_lines_are_errors() {
        let s = server();
        let a = ask(&s, r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#);
        assert_eq!(a["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(a["id"], 6);
        let a = ask(&s, "{not json");
        assert_eq!(a["error"]["code"], PARSE_ERROR);
        assert_eq!(a["id"], Value::Null);
        let a = ask(&s, r#"[{"jsonrpc":"2.0","id":7,"method":"ping"}]"#);
        assert_eq!(a["error"]["code"], INVALID_REQUEST);
        assert_eq!(ask(&s, r#"{"jsonrpc":"2.0","id":8,"method":"ping"}"#)["result"], json!({}));
    }

    #[test]
    fn serve_answers_line_by_line_until_input_ends() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"move"}}"#,
            "\n",
        );
        let mut out = Vec::new();
        server().serve(input.as_bytes(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        let lines: Vec<Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2, "one answer per request, none for the notification: {out}");
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[1]["result"]["content"][0]["text"], "moved");
    }
}
