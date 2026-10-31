use super::*;

fn root(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ito-agent-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn app(root: &Path) -> App {
    App {
        name: "demo".into(),
        skill: "---\nname: demo\n---\nUse demo.\n".into(),
        mcp: Some(Launch::new("demo", &["mcp"])),
        hook: Some("demo hook session-start".into()),
        manifest: root.join("state").join("agent-install.json"),
    }
}

fn both(root: &Path) -> Home {
    let home = Home::under(&root.join("home"));
    fs::create_dir_all(&home.claude_dir).unwrap();
    fs::create_dir_all(&home.codex_dir).unwrap();
    home
}

fn json_of(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn states(app: &App, home: &Home) -> Vec<(Agent, Item, State)> {
    status(app, home, &Options::default()).unwrap().into_iter().map(|s| (s.agent, s.item, s.state)).collect()
}

#[test]
fn setup_installs_everything_and_a_second_run_changes_nothing() {
    let r = root("idem");
    let (app, home) = (app(&r), both(&r));
    let first = setup(&app, &home, &Options::default()).unwrap();
    assert!(first.changed);
    assert!(states(&app, &home).iter().all(|s| s.2 == State::Installed), "{:?}", states(&app, &home));
    assert_eq!(fs::read_to_string(home.claude_dir.join("skills/demo/SKILL.md")).unwrap(), app.skill);
    assert_eq!(json_of(&home.claude_json)["mcpServers"]["demo"]["command"], "demo");
    assert_eq!(
        json_of(&home.claude_dir.join("settings.json"))["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        "demo hook session-start"
    );
    assert!(fs::read_to_string(home.codex_dir.join("config.toml")).unwrap().contains("[mcp_servers.demo]"));
    let second = setup(&app, &home, &Options::default()).unwrap();
    assert!(!second.changed, "{:?}", second.lines);
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn other_keys_servers_and_hooks_are_kept_in_order() {
    let r = root("keep");
    let (app, home) = (app(&r), both(&r));
    fs::write(&home.claude_json, r#"{"zeta":1,"mcpServers":{"other":{"command":"x"}},"alpha":2}"#).unwrap();
    fs::write(
        home.claude_dir.join("settings.json"),
        r#"{"theme":"dark","hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"mine"}]}]}}"#,
    )
    .unwrap();
    fs::write(home.codex_dir.join("config.toml"), "# my notes\nmodel = \"x\"\n").unwrap();
    setup(&app, &home, &Options::default()).unwrap();
    let j = json_of(&home.claude_json);
    assert_eq!(j["mcpServers"]["other"]["command"], "x");
    assert_eq!(j.as_object().unwrap().keys().collect::<Vec<_>>(), ["zeta", "mcpServers", "alpha"]);
    let s = json_of(&home.claude_dir.join("settings.json"));
    assert_eq!(s["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
    assert_eq!(s["hooks"]["SessionStart"][0]["hooks"][0]["command"], "mine");
    let c = fs::read_to_string(home.codex_dir.join("config.toml")).unwrap();
    assert!(c.starts_with("# my notes\nmodel = \"x\"\n"), "{c}");
    remove(&app, &home, false).unwrap();
    let s = json_of(&home.claude_dir.join("settings.json"));
    assert_eq!(s["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
    assert!(json_of(&home.claude_json)["mcpServers"].get("demo").is_none());
    assert_eq!(json_of(&home.claude_json)["mcpServers"]["other"]["command"], "x");
    assert_eq!(fs::read_to_string(home.codex_dir.join("config.toml")).unwrap(), "# my notes\nmodel = \"x\"\n");
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn dry_run_writes_nothing() {
    let r = root("dry");
    let (app, home) = (app(&r), both(&r));
    let report = setup(&app, &home, &Options { dry_run: true, ..Options::default() }).unwrap();
    assert!(report.changed);
    assert!(!home.claude_json.exists() && !app.manifest.exists() && !home.claude_dir.join("skills").exists());
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn an_old_skill_is_updated_but_an_edited_one_is_left() {
    let r = root("stale");
    let (mut app, home) = (app(&r), both(&r));
    setup(&app, &home, &Options::default()).unwrap();
    let skill = home.claude_dir.join("skills/demo/SKILL.md");
    app.skill = "new text\n".into();
    assert_eq!(states(&app, &home)[0].2, State::Stale);
    setup(&app, &home, &Options::default()).unwrap();
    assert_eq!(fs::read_to_string(&skill).unwrap(), "new text\n");
    fs::write(&skill, "mine\n").unwrap();
    app.skill = "newer\n".into();
    assert_eq!(states(&app, &home)[0].2, State::Edited);
    let report = setup(&app, &home, &Options::default()).unwrap();
    assert_eq!(fs::read_to_string(&skill).unwrap(), "mine\n");
    assert!(report.lines.iter().any(|l| l.contains("you edited")));
    setup(&app, &home, &Options { force: true, ..Options::default() }).unwrap();
    assert_eq!(fs::read_to_string(&skill).unwrap(), "newer\n");
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn remove_takes_back_only_ours_and_leaves_edits() {
    let r = root("remove");
    let (app, home) = (app(&r), both(&r));
    setup(&app, &home, &Options::default()).unwrap();
    let skill = home.claude_dir.join("skills/demo/SKILL.md");
    fs::write(&skill, "mine\n").unwrap();
    let report = remove(&app, &home, false).unwrap();
    assert!(report.lines.iter().any(|l| l.contains("claude skill: left")), "{:?}", report.lines);
    assert!(skill.exists());
    assert!(!json_of(&home.claude_json)["mcpServers"].as_object().unwrap().contains_key("demo"));
    assert!(!app.manifest.exists());
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn remove_cleans_up_the_skill_folder_and_the_manifest() {
    let r = root("clean");
    let (app, home) = (app(&r), both(&r));
    setup(&app, &home, &Options::default()).unwrap();
    remove(&app, &home, false).unwrap();
    assert!(!home.claude_dir.join("skills/demo").exists());
    assert!(!app.manifest.exists());
    assert!(json_of(&home.claude_dir.join("settings.json")).get("hooks").is_none());
    assert!(!remove(&app, &home, false).unwrap().changed);
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn no_hook_skips_it_and_takes_back_one_we_added() {
    let r = root("nohook");
    let (app, home) = (app(&r), both(&r));
    setup(&app, &home, &Options { hook: false, ..Options::default() }).unwrap();
    assert!(!home.claude_dir.join("settings.json").exists());
    setup(&app, &home, &Options::default()).unwrap();
    assert!(home.claude_dir.join("settings.json").exists());
    setup(&app, &home, &Options { hook: false, ..Options::default() }).unwrap();
    assert!(json_of(&home.claude_dir.join("settings.json")).get("hooks").is_none());
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn an_agent_that_is_not_there_is_skipped_unless_asked() {
    let r = root("absent");
    let app = app(&r);
    let home = Home::under(&r.join("home"));
    let report = setup(&app, &home, &Options::default()).unwrap();
    assert!(!report.changed && !home.claude_json.exists());
    setup(&app, &home, &Options { agents: vec![Agent::Codex], ..Options::default() }).unwrap();
    assert!(home.codex_dir.join("config.toml").exists() && !home.claude_json.exists());
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn a_broken_user_file_is_an_error_not_overwritten() {
    let r = root("broken");
    let (app, home) = (app(&r), both(&r));
    fs::write(&home.claude_json, "{ not json").unwrap();
    assert!(setup(&app, &home, &Options::default()).is_err());
    assert_eq!(fs::read_to_string(&home.claude_json).unwrap(), "{ not json");
    let _ = fs::remove_dir_all(&r);
}

#[test]
fn a_codex_server_with_extra_keys_counts_as_edited() {
    let r = root("codex-edit");
    let (app, home) = (app(&r), both(&r));
    fs::write(home.codex_dir.join("config.toml"), "[mcp_servers.demo]\ncommand = \"demo\"\nargs = [\"mcp\"]\nenv = { A = \"1\" }\n")
        .unwrap();
    let rows = states(&app, &home);
    assert!(rows.contains(&(Agent::Codex, Item::Mcp, State::Edited)), "{rows:?}");
    let _ = fs::remove_dir_all(&r);
}
