//! Codex's `~/.codex/config.toml`: `[mcp_servers.<app>]` is added or taken away with `toml_edit`, so the rest of
//! the file keeps its comments and layout.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::{json, Value};
use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::{invalid, write_atomic, Launch};

/// The server as compared and hashed: JSON of its command and arguments.
pub(crate) fn mcp_text(launch: &Launch) -> String {
    json!({ "command": launch.command, "args": launch.args }).to_string()
}

fn load(path: &Path) -> io::Result<DocumentMut> {
    match fs::read_to_string(path) {
        Ok(t) => t.parse::<DocumentMut>().map_err(|e| invalid(path, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(e) => Err(e),
    }
}

pub(crate) fn read_mcp(path: &Path, name: &str) -> io::Result<Option<String>> {
    let doc = load(path)?;
    let Some(table) = doc.get("mcp_servers").and_then(Item::as_table_like).and_then(|s| s.get(name)).and_then(Item::as_table_like) else {
        return Ok(None);
    };
    let command = table.get("command").and_then(Item::as_str).unwrap_or_default();
    let args: Vec<&str> =
        table.get("args").and_then(Item::as_array).map(|a| a.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    let mut text = json!({ "command": command, "args": args }).to_string();
    if table.iter().any(|(k, _)| k != "command" && k != "args") {
        text.push_str("+other keys"); // something the user added: never equal to what we write
    }
    Ok(Some(text))
}

pub(crate) fn write_mcp(path: &Path, name: &str, want: &str) -> io::Result<()> {
    let launch: Value = serde_json::from_str(want).map_err(io::Error::other)?;
    let mut doc = load(path)?;
    if !doc.contains_key("mcp_servers") {
        let mut t = Table::new();
        t.set_implicit(true);
        doc.insert("mcp_servers", Item::Table(t));
    }
    let servers = doc["mcp_servers"].as_table_mut().ok_or_else(|| invalid(path, "`mcp_servers` is not a table"))?;
    let mut t = Table::new();
    t.insert("command", value(launch["command"].as_str().unwrap_or_default()));
    let mut args = Array::new();
    for a in launch["args"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        args.push(a);
    }
    t.insert("args", value(args));
    servers.insert(name, Item::Table(t));
    write_atomic(path, &doc.to_string())
}

pub(crate) fn remove_mcp(path: &Path, name: &str) -> io::Result<()> {
    let mut doc = load(path)?;
    if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut) {
        servers.remove(name);
    }
    write_atomic(path, &doc.to_string())
}
