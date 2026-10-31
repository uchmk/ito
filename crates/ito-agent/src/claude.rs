//! Claude Code's files: `~/.claude.json` (MCP servers) and `~/.claude/settings.json` (hooks), both JSON the user
//! also edits, so a key is added or taken away and the rest stays as it was, in its order.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::{hook_group_value, invalid, write_atomic, Launch};

pub(crate) fn mcp_value(launch: &Launch) -> Value {
    json!({ "type": "stdio", "command": launch.command, "args": launch.args, "env": {} })
}

pub(crate) fn hook_group(command: &str) -> Value {
    hook_group_value(command)
}

fn load(path: &Path) -> io::Result<Map<String, Value>> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e),
    };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(&text).map_err(|e| invalid(path, e))? {
        Value::Object(m) => Ok(m),
        _ => Err(invalid(path, "not a JSON object")),
    }
}

fn save(path: &Path, map: Map<String, Value>) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(&Value::Object(map)).map_err(io::Error::other)?;
    text.push('\n');
    write_atomic(path, &text)
}

fn object<'a>(map: &'a mut Map<String, Value>, key: &str, path: &Path) -> io::Result<&'a mut Map<String, Value>> {
    map.entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| invalid(path, format!("`{key}` is not an object")))
}

pub(crate) fn read_mcp(path: &Path, name: &str) -> io::Result<Option<String>> {
    Ok(load(path)?.get("mcpServers").and_then(|s| s.get(name)).map(|v| v.to_string()))
}

pub(crate) fn write_mcp(path: &Path, name: &str, want: &str) -> io::Result<()> {
    let mut map = load(path)?;
    let value: Value = serde_json::from_str(want).map_err(io::Error::other)?;
    object(&mut map, "mcpServers", path)?.insert(name.to_owned(), value);
    save(path, map)
}

pub(crate) fn remove_mcp(path: &Path, name: &str) -> io::Result<()> {
    let mut map = load(path)?;
    if let Some(servers) = map.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.shift_remove(name);
    }
    save(path, map)
}

/// Whether a SessionStart group holds one of the commands.
fn is_ours(group: &Value, ids: &[&str]) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hs| hs.iter().any(|h| h.get("command").and_then(Value::as_str).is_some_and(|c| ids.contains(&c))))
}

fn session_start<'a>(map: &'a mut Map<String, Value>, path: &Path) -> io::Result<&'a mut Vec<Value>> {
    object(map, "hooks", path)?
        .entry("SessionStart")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| invalid(path, "`hooks.SessionStart` is not an array"))
}

pub(crate) fn read_hook(path: &Path, ids: &[&str]) -> io::Result<Option<String>> {
    Ok(load(path)?
        .get("hooks")
        .and_then(|h| h.get("SessionStart"))
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|g| is_ours(g, ids)))
        .map(|g| g.to_string()))
}

pub(crate) fn write_hook(path: &Path, ids: &[&str], want: &str) -> io::Result<()> {
    let mut map = load(path)?;
    let group: Value = serde_json::from_str(want).map_err(io::Error::other)?;
    let groups = session_start(&mut map, path)?;
    match groups.iter().position(|g| is_ours(g, ids)) {
        Some(i) => groups[i] = group,
        None => groups.push(group),
    }
    save(path, map)
}

pub(crate) fn remove_hook(path: &Path, ids: &[&str]) -> io::Result<()> {
    let mut map = load(path)?;
    let Some(hooks) = map.get_mut("hooks").and_then(Value::as_object_mut) else { return Ok(()) };
    if let Some(groups) = hooks.get_mut("SessionStart").and_then(Value::as_array_mut) {
        groups.retain(|g| !is_ours(g, ids));
        if groups.is_empty() {
            hooks.shift_remove("SessionStart");
        }
    }
    if hooks.is_empty() {
        map.shift_remove("hooks");
    }
    save(path, map)
}
