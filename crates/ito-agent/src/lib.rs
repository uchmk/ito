//! Teaching AI coding agents about an app (`<app> skill setup|status|remove`).
//!
//! For each agent found on the PC, [`setup`] writes the app's skill file, registers its MCP server and adds a
//! SessionStart hook that tells the agent where it is. What it wrote is recorded in a manifest in the app's state
//! folder, so [`remove`] takes back only that, and [`status`] can tell a file the user edited from one the app
//! would now write differently.
//!
//! | agent | skill | MCP server | hook |
//! |---|---|---|---|
//! | [`Agent::Claude`] | `~/.claude/skills/<app>/SKILL.md` | `mcpServers.<app>` in `~/.claude.json` | `hooks.SessionStart` in `~/.claude/settings.json` |
//! | [`Agent::Codex`] | (none) | `[mcp_servers.<app>]` in `~/.codex/config.toml` | (none) |
//!
//! - **The app owns the words**: the skill text, the command that starts its MCP server and the hook command are
//!   in [`App`]. This crate owns where they go, the formats, the manifest and the merging.
//! - **Idempotent**: running [`setup`] twice changes nothing the second time. Other servers, other hooks and every
//!   other key of the user's files are kept, in their order (a Codex file also keeps its comments).
//! - **Nothing the user edited is overwritten** unless [`Options::force`]; [`remove`] leaves it and says so.
//! - **`dry_run`** returns the same [`Report`] and writes nothing.
//! - **Nothing is printed here**: the app prints [`Report::lines`] (through `ito-console`).
//! - **Tests** point everything at a temp folder: [`Home::under`], or the environment `ITO_AGENT_HOME` for
//!   [`Home::detect`].

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

mod claude;
mod codex;

/// The environment variable that replaces the home folder, for tests.
pub const HOME_ENV: &str = "ITO_AGENT_HOME";

/// An AI coding agent this crate knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub const ALL: [Agent; 2] = [Agent::Claude, Agent::Codex];

    pub fn name(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }
}

/// One thing that is installed for an agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Skill,
    Mcp,
    Hook,
}

impl Item {
    pub fn name(self) -> &'static str {
        match self {
            Item::Skill => "skill",
            Item::Mcp => "mcp",
            Item::Hook => "hook",
        }
    }
}

/// Where an item stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Not there.
    Missing,
    /// Exactly what the app asks for now.
    Installed,
    /// Written by this app earlier, untouched since, and the app would write something else now.
    Stale,
    /// There, and not what this app wrote (the user changed it, or it was never ours).
    Edited,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Missing => "missing",
            State::Installed => "installed",
            State::Stale => "stale",
            State::Edited => "edited-by-user",
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a program is started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
}

impl Launch {
    pub fn new(command: impl Into<String>, args: &[&str]) -> Self {
        Self { command: command.into(), args: args.iter().map(|a| a.to_string()).collect() }
    }
}

/// What the app asks to be installed.
#[derive(Clone, Debug)]
pub struct App {
    /// The app's name: the skill folder, the MCP server's name.
    pub name: String,
    /// The text of `SKILL.md`, front matter and all.
    pub skill: String,
    /// The MCP server (`<app> mcp`); `None` registers none.
    pub mcp: Option<Launch>,
    /// The SessionStart hook's command line as a shell runs it (`<app> hook session-start`); `None` adds none.
    pub hook: Option<String>,
    /// Where the manifest is kept (`<state folder>/agent-install.json`).
    pub manifest: PathBuf,
}

/// Where the agents keep their files.
#[derive(Clone, Debug)]
pub struct Home {
    /// `~/.claude`
    pub claude_dir: PathBuf,
    /// `~/.claude.json`, where the user-scope MCP servers are.
    pub claude_json: PathBuf,
    /// `~/.codex` (`$CODEX_HOME`)
    pub codex_dir: PathBuf,
}

impl Home {
    /// Everything under `root`, as under a home folder.
    pub fn under(root: &Path) -> Self {
        Self { claude_dir: root.join(".claude"), claude_json: root.join(".claude.json"), codex_dir: root.join(".codex") }
    }

    /// The real one: `ITO_AGENT_HOME`, else the user's home folder; Codex's folder follows `CODEX_HOME`.
    pub fn detect() -> io::Result<Self> {
        let root = [HOME_ENV, "HOME", "USERPROFILE"]
            .iter()
            .find_map(|k| std::env::var_os(k).filter(|v| !v.is_empty()))
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("no home folder"))?;
        let mut home = Self::under(&root);
        if std::env::var_os(HOME_ENV).is_none() {
            if let Some(c) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
                home.codex_dir = PathBuf::from(c);
            }
        }
        Ok(home)
    }

    /// Whether the agent seems to be in use on this PC.
    pub fn has(&self, agent: Agent) -> bool {
        match agent {
            Agent::Claude => self.claude_dir.exists() || self.claude_json.exists(),
            Agent::Codex => self.codex_dir.exists(),
        }
    }
}

/// What [`setup`] is asked.
#[derive(Clone, Debug)]
pub struct Options {
    /// Plan only; write nothing.
    pub dry_run: bool,
    /// Add the SessionStart hook (`--no-hook` turns it off, and takes back one this app added).
    pub hook: bool,
    /// Replace what the user edited.
    pub force: bool,
    /// Which agents; empty means every one found on the PC.
    pub agents: Vec<Agent>,
}

impl Default for Options {
    fn default() -> Self {
        Self { dry_run: false, hook: true, force: false, agents: Vec::new() }
    }
}

/// What was done, or with `dry_run` what would be.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// One line each, for the app to print.
    pub lines: Vec<String>,
    /// Whether anything is (or would be) written or taken back.
    pub changed: bool,
}

impl Report {
    fn line(&mut self, text: String) {
        self.lines.push(text);
    }

    fn change(&mut self, text: String) {
        self.changed = true;
        self.lines.push(text);
    }
}

/// One row of [`status`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub agent: Agent,
    pub item: Item,
    pub state: State,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    agent: String,
    item: String,
    /// What finds it again: the hook's command line.
    id: String,
    /// The SHA-256 of what was written.
    sha: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    entries: Vec<Entry>,
}

impl Manifest {
    fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| invalid(path, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        if self.entries.is_empty() {
            return match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        let mut text = serde_json::to_string_pretty(&Manifest { version: 1, entries: self.entries.clone() }).map_err(io::Error::other)?;
        text.push('\n');
        write_atomic(path, &text)
    }

    fn get(&self, agent: Agent, item: Item) -> Option<&Entry> {
        self.entries.iter().find(|e| e.agent == agent.name() && e.item == item.name())
    }

    fn set(&mut self, agent: Agent, item: Item, id: String, sha: String) {
        self.entries.retain(|e| !(e.agent == agent.name() && e.item == item.name()));
        self.entries.push(Entry { agent: agent.name().into(), item: item.name().into(), id, sha });
    }

    fn drop(&mut self, agent: Agent, item: Item) {
        self.entries.retain(|e| !(e.agent == agent.name() && e.item == item.name()));
    }
}

pub(crate) fn invalid(path: &Path, why: impl fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("{}: {why}", path.display()))
}

pub(crate) fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Write beside, then rename over: a reader never sees half a file. Parents are made.
pub(crate) fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.ito-agent.tmp"));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn skill_path(home: &Home, app: &App) -> PathBuf {
    home.claude_dir.join("skills").join(&app.name).join("SKILL.md")
}

/// What the app asks for, as the text compared and hashed. `None`: this agent has no such thing, or the app
/// does not ask.
fn wanted(app: &App, agent: Agent, item: Item, hook: bool) -> Option<String> {
    match (agent, item) {
        (Agent::Claude, Item::Skill) => Some(app.skill.clone()),
        (Agent::Claude, Item::Mcp) => app.mcp.as_ref().map(claude::mcp_value).map(|v| v.to_string()),
        (Agent::Claude, Item::Hook) => app.hook.as_ref().filter(|_| hook).map(|c| claude::hook_group(c).to_string()),
        (Agent::Codex, Item::Mcp) => app.mcp.as_ref().map(codex::mcp_text),
        (Agent::Codex, _) => None,
    }
}

/// The hook's identity: its command line.
fn id_of(app: &App, agent: Agent, item: Item) -> String {
    match (agent, item) {
        (Agent::Claude, Item::Hook) => app.hook.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// What is in the agent's files now. `ids` are the command lines that count as this app's hook.
fn current(home: &Home, app: &App, agent: Agent, item: Item, ids: &[&str]) -> io::Result<Option<String>> {
    match (agent, item) {
        (Agent::Claude, Item::Skill) => match fs::read_to_string(skill_path(home, app)) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        },
        (Agent::Claude, Item::Mcp) => claude::read_mcp(&home.claude_json, &app.name),
        (Agent::Claude, Item::Hook) => claude::read_hook(&home.claude_dir.join("settings.json"), ids),
        (Agent::Codex, Item::Mcp) => codex::read_mcp(&home.codex_dir.join("config.toml"), &app.name),
        (Agent::Codex, _) => Ok(None),
    }
}

fn apply(home: &Home, app: &App, agent: Agent, item: Item, ids: &[&str], hook: bool) -> io::Result<()> {
    let want = wanted(app, agent, item, hook).ok_or_else(|| io::Error::other("nothing to write"))?;
    match (agent, item) {
        (Agent::Claude, Item::Skill) => write_atomic(&skill_path(home, app), &want),
        (Agent::Claude, Item::Mcp) => claude::write_mcp(&home.claude_json, &app.name, &want),
        (Agent::Claude, Item::Hook) => claude::write_hook(&home.claude_dir.join("settings.json"), ids, &want),
        (Agent::Codex, Item::Mcp) => codex::write_mcp(&home.codex_dir.join("config.toml"), &app.name, &want),
        (Agent::Codex, _) => Ok(()),
    }
}

fn erase(home: &Home, app: &App, agent: Agent, item: Item, ids: &[&str]) -> io::Result<()> {
    match (agent, item) {
        (Agent::Claude, Item::Skill) => {
            let path = skill_path(home, app);
            match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            if let Some(dir) = path.parent() {
                let _ = fs::remove_dir(dir); // only when empty
            }
            Ok(())
        }
        (Agent::Claude, Item::Mcp) => claude::remove_mcp(&home.claude_json, &app.name),
        (Agent::Claude, Item::Hook) => claude::remove_hook(&home.claude_dir.join("settings.json"), ids),
        (Agent::Codex, Item::Mcp) => codex::remove_mcp(&home.codex_dir.join("config.toml"), &app.name),
        (Agent::Codex, _) => Ok(()),
    }
}

fn classify(cur: Option<&str>, want: &str, rec: Option<&Entry>) -> State {
    match cur {
        None => State::Missing,
        Some(c) if c == want => State::Installed,
        Some(c) if rec.is_some_and(|e| e.sha == sha(c)) => State::Stale,
        Some(_) => State::Edited,
    }
}

fn label(agent: Agent, item: Item) -> String {
    format!("{} {}", agent.name(), item.name())
}

fn agents_of(home: &Home, asked: &[Agent]) -> Vec<Agent> {
    Agent::ALL.into_iter().filter(|a| if asked.is_empty() { home.has(*a) } else { asked.contains(a) }).collect()
}

/// The command lines that find this app's hook: the one it asks for and the one it recorded.
fn hook_ids<'a>(app: &'a App, rec: Option<&'a Entry>) -> Vec<&'a str> {
    let mut ids: Vec<&str> = rec.map(|e| e.id.as_str()).filter(|i| !i.is_empty()).into_iter().collect();
    if let Some(c) = app.hook.as_deref() {
        if !ids.contains(&c) {
            ids.push(c);
        }
    }
    ids
}

/// Install what the app asks for in each agent, leaving what the user edited.
pub fn setup(app: &App, home: &Home, opts: &Options) -> io::Result<Report> {
    let mut report = Report::default();
    let mut manifest = Manifest::load(&app.manifest)?;
    let before = manifest.entries.clone();
    let agents = agents_of(home, &opts.agents);
    if agents.is_empty() {
        report.line("no AI agent found (looked for Claude Code and Codex)".into());
    }
    for agent in agents {
        for item in [Item::Skill, Item::Mcp, Item::Hook] {
            if agent == Agent::Codex && item != Item::Mcp {
                continue;
            }
            let rec = manifest.get(agent, item).cloned();
            let ids = hook_ids(app, rec.as_ref());
            let ids: &[&str] = if item == Item::Hook { &ids } else { &[] };
            let cur = current(home, app, agent, item, ids)?;
            let name = label(agent, item);
            let Some(want) = wanted(app, agent, item, opts.hook) else {
                // No longer asked for: take back what this app added, if untouched.
                if let Some(rec) = &rec {
                    if cur.as_deref().is_some_and(|c| sha(c) == rec.sha) {
                        if !opts.dry_run {
                            erase(home, app, agent, item, ids)?;
                        }
                        report.change(format!("{name}: removed (no longer asked for)"));
                    } else if cur.is_some() {
                        report.line(format!("{name}: left, you edited it"));
                    }
                    manifest.drop(agent, item);
                }
                continue;
            };
            match classify(cur.as_deref(), &want, rec.as_ref()) {
                State::Installed => {
                    report.line(format!("{name}: already installed"));
                    manifest.set(agent, item, id_of(app, agent, item), sha(&want));
                }
                state @ (State::Missing | State::Stale) => {
                    if !opts.dry_run {
                        apply(home, app, agent, item, ids, opts.hook)?;
                    }
                    let verb = if state == State::Missing { "installed" } else { "updated" };
                    report.change(format!("{name}: {verb}"));
                    manifest.set(agent, item, id_of(app, agent, item), sha(&want));
                }
                State::Edited if opts.force => {
                    if !opts.dry_run {
                        apply(home, app, agent, item, ids, opts.hook)?;
                    }
                    report.change(format!("{name}: replaced what you edited"));
                    manifest.set(agent, item, id_of(app, agent, item), sha(&want));
                }
                State::Edited => report.line(format!("{name}: left, you edited it (force replaces it)")),
            }
        }
    }
    if !opts.dry_run && manifest.entries != before {
        manifest.save(&app.manifest)?;
    }
    if opts.dry_run {
        report.line("dry run: nothing was written".into());
    }
    Ok(report)
}

/// Where each item stands, for the agents found on the PC and those the manifest remembers.
pub fn status(app: &App, home: &Home, opts: &Options) -> io::Result<Vec<Status>> {
    let manifest = Manifest::load(&app.manifest)?;
    let mut rows = Vec::new();
    for agent in agents_of(home, &opts.agents) {
        for item in [Item::Skill, Item::Mcp, Item::Hook] {
            let rec = manifest.get(agent, item);
            let ids = hook_ids(app, rec);
            let ids: &[&str] = if item == Item::Hook { &ids } else { &[] };
            let Some(want) = wanted(app, agent, item, opts.hook) else { continue };
            let cur = current(home, app, agent, item, ids)?;
            rows.push(Status { agent, item, state: classify(cur.as_deref(), &want, rec) });
        }
    }
    Ok(rows)
}

/// Take back what [`setup`] installed, and only that. What the user edited since is left, and said so.
pub fn remove(app: &App, home: &Home, dry_run: bool) -> io::Result<Report> {
    let mut report = Report::default();
    let mut manifest = Manifest::load(&app.manifest)?;
    if manifest.entries.is_empty() {
        report.line("nothing was installed by this app".into());
        return Ok(report);
    }
    for rec in manifest.entries.clone() {
        let (Some(agent), Some(item)) = (
            Agent::ALL.into_iter().find(|a| a.name() == rec.agent),
            [Item::Skill, Item::Mcp, Item::Hook].into_iter().find(|i| i.name() == rec.item),
        ) else {
            continue;
        };
        let ids = [rec.id.as_str()];
        let ids: &[&str] = if item == Item::Hook { &ids } else { &[] };
        let name = label(agent, item);
        match current(home, app, agent, item, ids)? {
            None => report.line(format!("{name}: already gone")),
            Some(c) if sha(&c) == rec.sha => {
                if !dry_run {
                    erase(home, app, agent, item, ids)?;
                }
                report.change(format!("{name}: removed"));
            }
            Some(_) => report.line(format!("{name}: left, you edited it")),
        }
        manifest.drop(agent, item);
    }
    if !dry_run {
        manifest.save(&app.manifest)?;
    } else {
        report.line("dry run: nothing was written".into());
    }
    Ok(report)
}

pub(crate) fn hook_group_value(command: &str) -> Value {
    json!({ "hooks": [{ "type": "command", "command": command }] })
}

#[cfg(test)]
mod tests;
