use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::adapter::scratch_adapter;
use crate::cli_stub::CliStub;
use crate::dto::{ErrorKind, ItemScope};
use crate::install_source::SourceKind;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn skill(dir: &Path, name: &str) {
    write(
        &dir.join(name).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: Acme\n---\n"),
    );
}

fn muse_plugin(home: &Path, id: &str, enabled: bool) -> String {
    let package =
        crate::paths::muse_data_for(home).join(format!("plugins/cache/local/{id}/abc/package"));
    write(
        &package.join(".muse-plugin/plugin.json"),
        &format!(r#"{{"name":"{id}","version":"1.0.0"}}"#),
    );
    skill(&package.join("skills"), &format!("{id}-review"));
    serde_json::json!({
        "id": id,
        "version": "1.0.0",
        "enabled": enabled,
        "source": { "path": format!("/srv/acme/{id}") },
        "cache_path": package,
    })
    .to_string()
}

fn seed(id: AgentId, home: &Path) {
    match id {
        AgentId::Claude => {
            let root = home.join(".claude");
            let plugin = root.join("plugins/cache/workshop/workbench/1.0.0");
            skill(&plugin.join("skills"), "brainstorming");
            write(&plugin.join(".mcp.json"), r#"{"docs":{"command":"docs"}}"#);
            let marketplace = root.join("plugins/marketplaces/workshop");
            write(
                &marketplace.join(".claude-plugin/marketplace.json"),
                r#"{"plugins":[{"name":"workbench","version":"1.1.0","source":"./plugins/workbench"}]}"#,
            );
            write(
                &root.join("plugins/known_marketplaces.json"),
                &serde_json::json!({ "workshop": {
                    "source": { "repo": "acme/workshop" },
                    "installLocation": marketplace,
                } })
                .to_string(),
            );
            write(
                &root.join("plugins/installed_plugins.json"),
                &serde_json::json!({ "version": 2, "plugins": { "workbench@workshop": [
                    { "scope": "user", "installPath": plugin, "version": "1.0.0" }
                ] } })
                .to_string(),
            );
            write(
                &root.join("settings.json"),
                r#"{"enabledPlugins":{"workbench@workshop":true}}"#,
            );
            skill(&root.join("skills"), "statusline");
            write(
                &root.join(".claude.json"),
                r#"{"mcpServers":{"github":{"command":"npx"}}}"#,
            );
        }
        AgentId::Codex => {
            let root = home.join(".codex");
            write(
                &root.join("plugins/cache/workshop/workbench/0.22.1/.codex-plugin/plugin.json"),
                r#"{"name":"workbench","version":"0.22.1"}"#,
            );
            write(
                &root.join(".tmp/marketplaces/workshop/.claude-plugin/marketplace.json"),
                r#"{"plugins":[{"name":"workbench","version":"0.23.0","source":"./plugins/workbench"}]}"#,
            );
            skill(&root.join("skills"), "loom");
            write(
                &root.join("config.toml"),
                "[marketplaces.workshop]\nsource_type = \"git\"\nsource = \"https://github.com/acme/workshop.git\"\n\n[plugins.\"workbench@workshop\"]\nenabled = true\n\n[mcp_servers.github]\ncommand = \"npx\"\n",
            );
        }
        AgentId::Antigravity => {
            let gemini = home.join(".gemini");
            write(
                &gemini.join("antigravity-cli/plugins/alpha/plugin.json"),
                r#"{"name":"alpha"}"#,
            );
            write(
                &gemini.join("config/plugins/beta/plugin.json"),
                r#"{"name":"beta"}"#,
            );
            write(
                &gemini.join("antigravity-cli/config.json"),
                r#"{"plugins":{"alpha":{"enabled":true}}}"#,
            );
            write(
                &gemini.join("config/mcp_config.json"),
                r#"{"mcpServers":{"github":{"command":"npx"}}}"#,
            );
            write(&gemini.join("antigravity-cli/settings.json"), "{}");
            skill(&gemini.join("antigravity-cli/skills"), "solo");
        }
        AgentId::Cursor => {
            let root = home.join(".cursor");
            write(
                &root.join("plugins/local/acme/plugin.json"),
                r#"{"name":"acme"}"#,
            );
            skill(&root.join("skills"), "acme-notes");
            write(
                &root.join("mcp.json"),
                r#"{"mcpServers":{"github":{"command":"npx"}}}"#,
            );
        }
        AgentId::Muse => {
            let config = crate::paths::muse_config_for(home);
            let installed = format!(
                r#"{{"version":1,"plugins":{{"acme-tools":{},"quiet":{}}}}}"#,
                muse_plugin(home, "acme-tools", true),
                muse_plugin(home, "quiet", false)
            );
            write(
                &crate::paths::muse_data_for(home).join("plugins/installed.json"),
                &installed,
            );
            write(
                &config.join("settings.json"),
                r#"{"schema_version":1,"mcpServers":{"docs":{"url":"https://docs.example/mcp"}}}"#,
            );
            skill(&config.join("skills"), "acme-notes");
            write(
                &crate::paths::muse_data_for(home).join("plugins/marketplaces.json"),
                r#"{"marketplaces":{}}"#,
            );
        }
    }
}

fn contents(dir: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.insert(path.clone(), fs::read(&path).unwrap_or_default());
            }
        }
    }
    found
}

fn offline<T>(run: impl FnOnce() -> T) -> T {
    crate::drift::with_fetch_text(|url| panic!("a change read the network: {url}"), run)
}

static FETCHED: AtomicUsize = AtomicUsize::new(0);

fn count_fetch(_: &str) -> Result<Option<String>, crate::drift::Transient> {
    FETCHED.fetch_add(1, Ordering::SeqCst);
    Ok(None)
}

struct Home {
    id: AgentId,
    root: std::path::PathBuf,
    project: String,
    adapter: Box<dyn AgentAdapter>,
}

impl Home {
    fn new(id: AgentId) -> Self {
        let root = crate::paths::scratch_dir("on-n-off-change");
        seed(id, &root);
        let adapter = scratch_adapter(id, &root);
        let project = root.join("acme-project");
        let roots = adapter
            .item_roots(&ItemScope::Project {
                project_path: project.to_string_lossy().into_owned(),
            })
            .unwrap();
        skill(&roots.skills, "acme-local");
        Self {
            id,
            root,
            project: project.to_string_lossy().into_owned(),
            adapter,
        }
    }

    fn view(&self, scoped: bool) -> Option<&str> {
        scoped.then_some(self.project.as_str())
    }

    fn skill_id(&self, name: &str, scoped: bool) -> String {
        let tab = self.adapter.list_scope(self.view(scoped)).unwrap();
        tab.plugins
            .iter()
            .flat_map(|plugin| plugin.skills.iter())
            .chain(tab.user_skills.iter())
            .find(|skill| skill.name == name)
            .unwrap_or_else(|| panic!("{:?} lists no skill {name}", self.id))
            .id
            .clone()
    }

    fn backups(&self) -> std::path::PathBuf {
        self.root.join("_backups")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug)]
enum Expect {
    Made,
    Declared,
    Refused(String),
}

#[derive(Debug, Default)]
struct Applied {
    steps: usize,
    touched: Vec<std::path::PathBuf>,
    unsaved: Vec<std::path::PathBuf>,
    deadline: Option<Duration>,
}

fn faked(home: &Home, record: Arc<std::sync::Mutex<Applied>>) -> Changes {
    let (id, backups) = (home.id, home.backups());
    let skills = home.adapter.item_roots(&ItemScope::Global).unwrap().skills;
    Changes::faking(home.backups(), move |step, deadline| {
        let mut applied = record.lock().unwrap();
        applied.steps += 1;
        applied.deadline = Some(deadline);
        applied.touched = step.touches().to_vec();
        applied.unsaved = step
            .touches()
            .iter()
            .filter(|file| file.exists())
            .filter(|file| {
                let name = file.file_name().unwrap().to_string_lossy();
                crate::backup::kept(&backups, id, &name).is_empty()
            })
            .cloned()
            .collect();
        skill(&skills, "acme-made");
        Ok(())
    })
}

fn declared_reason(home: &Home, request: &Request) -> String {
    let declared = home.adapter.capabilities();
    match (request, request.declared(&declared)) {
        (Request::Install { source }, Change::Cli { .. } | Change::File { .. }) => {
            declared.admit(source).unwrap_err().message
        }
        (_, change) => change.refusal().message,
    }
}

fn long_running(request: &Request) -> bool {
    matches!(request, Request::Install { .. } | Request::Update { .. })
}

fn check(id: AgentId, scoped: bool, request: impl Fn(&Home) -> Request, expect: &Expect) {
    let home = Home::new(id);
    let request = request(&home);
    let what = format!("{id:?} {request:?} scoped={scoped}");
    let record = Arc::new(std::sync::Mutex::new(Applied::default()));
    let changes = faked(&home, Arc::clone(&record));
    let before = contents(&home.root);

    let answer = offline(|| changes.make(&*home.adapter, request.clone(), home.view(scoped)));

    let applied = record.lock().unwrap();
    match expect {
        Expect::Made => {
            let answer = answer.unwrap_or_else(|error| panic!("{what}: {error:?}"));
            assert_eq!(applied.steps, 1, "{what}");
            assert!(!applied.touched.is_empty(), "{what} touches nothing");
            assert!(
                applied.unsaved.is_empty(),
                "{what} ran before backing up {:?}",
                applied.unsaved
            );
            let deadline = if long_running(&request) {
                INSTALL_TIMEOUT
            } else {
                DEFAULT_TIMEOUT
            };
            assert_eq!(applied.deadline, Some(deadline), "{what}");
            let local = offline(|| {
                crate::drift::read(
                    &*home.adapter,
                    home.view(scoped),
                    crate::drift::Reach::Local,
                )
                .unwrap()
            });
            assert_eq!(answer, local, "{what}");
            assert!(
                answer
                    .user_skills
                    .iter()
                    .any(|skill| skill.name == "acme-made"),
                "{what}: the answer was read before the change"
            );
            assert_eq!(
                answer
                    .user_skills
                    .iter()
                    .any(|skill| skill.name == "acme-local"),
                scoped,
                "{what}: the answer is not the caller's view"
            );
        }
        Expect::Declared | Expect::Refused(_) => {
            let reason = match expect {
                Expect::Refused(reason) => reason.clone(),
                _ => declared_reason(&home, &request),
            };
            let error = answer.expect_err(&what);
            assert_eq!(error.message, reason, "{what}");
            assert_eq!(applied.steps, 0, "{what}");
            assert_eq!(contents(&home.root), before, "{what} wrote to the home");
        }
    }
}

fn named_skill(name: &'static str, scoped: bool) -> impl Fn(&Home) -> Request {
    move |home| Request::skill(&home.skill_id(name, scoped), false)
}

fn fixed(request: Request) -> impl Fn(&Home) -> Request {
    move |_| request.clone()
}

fn refused(reason: &str) -> Expect {
    Expect::Refused(reason.into())
}

#[test]
fn every_provider_checks_backs_up_and_answers_through_one_change_path() {
    use AgentId::{Antigravity, Claude, Codex, Cursor, Muse};
    use Expect::{Declared, Made};
    for scoped in [false, true] {
        check(
            Claude,
            scoped,
            fixed(Request::plugin("workbench@workshop", false)),
            &Made,
        );
        check(
            Claude,
            scoped,
            fixed(Request::plugin("acme@nowhere", false)),
            &refused("plugin not found: acme@nowhere"),
        );
        check(Claude, scoped, named_skill("statusline", scoped), &Made);
        check(
            Claude,
            scoped,
            fixed(Request::skill("workbench@workshop:brainstorming", false)),
            &refused("skill is not togglable: workbench@workshop:brainstorming"),
        );
        check(
            Claude,
            scoped,
            fixed(Request::skill("acme-missing", false)),
            &refused("skill not found: acme-missing"),
        );
        check(Claude, scoped, fixed(Request::mcp("github", false)), &Made);
        check(
            Claude,
            scoped,
            fixed(Request::mcp("plugin:workbench:docs", false)),
            &refused("mcp server is not togglable: plugin:workbench:docs"),
        );
        check(
            Claude,
            scoped,
            fixed(Request::mcp("acme-missing", false)),
            &refused("mcp server not found: acme-missing"),
        );
        check(
            Claude,
            scoped,
            fixed(Request::install("acme@webapp")),
            &Made,
        );
        check(
            Claude,
            scoped,
            fixed(Request::install("acme/webapp")),
            &Made,
        );
        check(
            Claude,
            scoped,
            fixed(Request::uninstall("workbench@workshop")),
            &Made,
        );
        check(
            Claude,
            scoped,
            fixed(Request::uninstall("acme@nowhere")),
            &refused("plugin not found: acme@nowhere"),
        );
        check(
            Claude,
            scoped,
            fixed(Request::update("workbench@workshop")),
            &Made,
        );
        check(
            Claude,
            scoped,
            fixed(Request::update("acme@nowhere")),
            &refused("plugin not found: acme@nowhere"),
        );

        check(
            Codex,
            scoped,
            fixed(Request::plugin("workbench@workshop", false)),
            &Made,
        );
        check(
            Codex,
            scoped,
            fixed(Request::plugin("acme@nowhere", false)),
            &refused("plugin not found: acme@nowhere"),
        );
        check(Codex, scoped, named_skill("loom", scoped), &Made);
        check(
            Codex,
            scoped,
            fixed(Request::skill("acme-missing", false)),
            &refused("skill not found: acme-missing"),
        );
        check(Codex, scoped, fixed(Request::mcp("github", false)), &Made);
        check(
            Codex,
            scoped,
            fixed(Request::mcp("acme-missing", false)),
            &refused("mcp server not found: acme-missing"),
        );
        check(Codex, scoped, fixed(Request::install("acme@webapp")), &Made);
        check(
            Codex,
            scoped,
            fixed(Request::uninstall("workbench@workshop")),
            &Made,
        );
        check(
            Codex,
            scoped,
            fixed(Request::update("workbench@workshop")),
            &Made,
        );

        check(
            Antigravity,
            scoped,
            fixed(Request::plugin("alpha@cli", false)),
            &Made,
        );
        check(
            Antigravity,
            scoped,
            fixed(Request::plugin("beta@config", false)),
            &refused("plugin is not togglable: beta@config"),
        );
        check(Antigravity, scoped, named_skill("solo", scoped), &Declared);
        check(
            Antigravity,
            scoped,
            fixed(Request::mcp("github", false)),
            &Made,
        );
        check(
            Antigravity,
            scoped,
            fixed(Request::install("acme@webapp")),
            &Made,
        );
        check(
            Antigravity,
            scoped,
            fixed(Request::uninstall("alpha@cli")),
            &Made,
        );
        check(
            Antigravity,
            scoped,
            fixed(Request::uninstall("beta@config")),
            &refused("plugin cannot be uninstalled here: beta@config"),
        );
        check(
            Antigravity,
            scoped,
            fixed(Request::update("alpha@cli")),
            &Declared,
        );

        check(
            Cursor,
            scoped,
            fixed(Request::plugin("acme@local", false)),
            &Declared,
        );
        check(Cursor, scoped, named_skill("acme-notes", scoped), &Declared);
        check(
            Cursor,
            scoped,
            fixed(Request::mcp("github", false)),
            &Declared,
        );
        check(
            Cursor,
            scoped,
            fixed(Request::install("acme@webapp")),
            &Declared,
        );
        check(
            Cursor,
            scoped,
            fixed(Request::uninstall("acme@local")),
            &Declared,
        );
        check(
            Cursor,
            scoped,
            fixed(Request::update("acme@local")),
            &Declared,
        );

        check(
            Muse,
            scoped,
            fixed(Request::plugin("acme-tools@local", false)),
            &Made,
        );
        check(
            Muse,
            scoped,
            fixed(Request::plugin("acme@nowhere", false)),
            &refused("plugin not found: acme@nowhere"),
        );
        check(Muse, scoped, named_skill("acme-notes", scoped), &Made);
        check(
            Muse,
            scoped,
            named_skill("acme-tools-review", scoped),
            &Made,
        );
        check(
            Muse,
            scoped,
            fixed(Request::skill("plugin:quiet:quiet-review", false)),
            &refused("skill is not togglable: plugin:quiet:quiet-review"),
        );
        check(Muse, scoped, fixed(Request::mcp("docs", false)), &Declared);
        check(Muse, scoped, fixed(Request::install("acme@webapp")), &Made);
        check(
            Muse,
            scoped,
            fixed(Request::install("acme/webapp")),
            &Declared,
        );
        check(
            Muse,
            scoped,
            fixed(Request::uninstall("acme-tools@local")),
            &Made,
        );
        check(
            Muse,
            scoped,
            fixed(Request::update("acme-tools@local")),
            &Made,
        );
    }
}

#[test]
fn a_skill_a_project_brings_is_listed_in_its_view_but_never_switched() {
    for id in [AgentId::Claude, AgentId::Codex, AgentId::Muse] {
        let home = Home::new(id);
        let local = home.skill_id("acme-local", true);
        assert!(local.starts_with("project:"), "{id:?} {local}");
        let record = Arc::new(std::sync::Mutex::new(Applied::default()));
        let changes = faked(&home, Arc::clone(&record));
        let before = contents(&home.root);
        let error = offline(|| {
            changes.make(
                &*home.adapter,
                Request::skill(&local, false),
                home.view(true),
            )
        })
        .unwrap_err();
        assert_eq!(error.message, format!("skill is not togglable: {local}"));
        assert_eq!(record.lock().unwrap().steps, 0, "{id:?}");
        assert_eq!(contents(&home.root), before, "{id:?}");
    }
}

#[test]
fn a_change_whose_backup_fails_is_refused_before_its_step_runs() {
    let cases: [(AgentId, Request); 6] = [
        (AgentId::Claude, Request::mcp("github", false)),
        (AgentId::Claude, Request::install("acme@webapp")),
        (AgentId::Codex, Request::plugin("workbench@workshop", false)),
        (AgentId::Antigravity, Request::plugin("alpha@cli", false)),
        (AgentId::Antigravity, Request::mcp("github", false)),
        (AgentId::Muse, Request::uninstall("acme-tools@local")),
    ];
    for (id, request) in cases {
        let home = Home::new(id);
        let record = Arc::new(std::sync::Mutex::new(Applied::default()));
        let changes = faked(&home, Arc::clone(&record));
        write(&home.backups(), "blocked");
        let before = contents(&home.root);
        let error = offline(|| changes.make(&*home.adapter, request.clone(), None))
            .expect_err(&format!("{id:?} {request:?}"));
        assert_eq!(error.kind, ErrorKind::Write, "{id:?} {request:?}");
        assert_eq!(record.lock().unwrap().steps, 0, "{id:?} {request:?}");
        assert_eq!(contents(&home.root), before, "{id:?} {request:?}");
    }
}

#[test]
fn the_fixtures_would_reach_the_network_if_a_change_read_the_enriched_view() {
    for id in [AgentId::Claude, AgentId::Codex] {
        let home = Home::new(id);
        let before = FETCHED.load(Ordering::SeqCst);
        let memo = crate::drift::Memo::new();
        crate::drift::with_fetch_text(count_fetch, || {
            crate::drift::read(&*home.adapter, None, crate::drift::Reach::Remote(&memo)).unwrap()
        });
        assert!(FETCHED.load(Ordering::SeqCst) > before, "{id:?}");
    }
}

#[test]
fn a_patch_backs_up_its_file_once_and_leaves_it_alone_when_its_edit_fails() {
    let home = Home::new(AgentId::Codex);
    let changes = Changes::at(home.backups());
    let config = home.root.join(".codex/config.toml");
    let original = fs::read_to_string(&config).unwrap();

    let failing = Step::toml(config.clone(), |doc| {
        doc.insert("acme", toml_edit::value("x"));
        Err(AdapterError::message("acme refused"))
    });
    let backups = changes
        .io
        .back_up(AgentId::Codex, failing.touches())
        .unwrap();
    let answer = failing
        .run(&changes.io, &backups, crate::cli::DEFAULT_TIMEOUT)
        .unwrap_err();
    assert_eq!(answer.message, "acme refused");
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    assert_eq!(
        crate::backup::kept(&home.backups(), AgentId::Codex, "config.toml").len(),
        1
    );

    changes
        .make(&*home.adapter, Request::mcp("github", false), None)
        .unwrap();
    assert_eq!(
        crate::backup::kept(&home.backups(), AgentId::Codex, "config.toml").len(),
        2
    );
}

#[test]
fn one_provider_makes_one_change_at_a_time() {
    let home = Home::new(AgentId::Codex);
    let busy = Arc::new(AtomicUsize::new(0));
    let overlapped = Arc::new(AtomicUsize::new(0));
    let (inside, seen) = (Arc::clone(&busy), Arc::clone(&overlapped));
    let changes = Changes::faking(home.backups(), move |_, _| {
        if inside.fetch_add(1, Ordering::SeqCst) > 0 {
            seen.fetch_add(1, Ordering::SeqCst);
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
        inside.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    });
    std::thread::scope(|scope| {
        for _ in 0..3 {
            scope.spawn(|| {
                changes
                    .make(&*home.adapter, Request::mcp("github", false), None)
                    .unwrap();
            });
        }
    });
    assert_eq!(overlapped.load(Ordering::SeqCst), 0);
}

#[test]
fn a_refused_source_kind_is_refused_in_the_providers_words() {
    let home = Home::new(AgentId::Muse);
    let declared = home.adapter.capabilities();
    let reason = declared.refused_sources[&SourceKind::GitMarketplace].clone();
    let error = Changes::at(home.backups())
        .make(
            &*home.adapter,
            Request::install("https://github.com/acme/webapp.git"),
            None,
        )
        .unwrap_err();
    assert_eq!(error.message, reason);
    assert!(!home.backups().exists());
}

#[test]
fn a_cli_step_is_stopped_at_the_deadline_it_is_run_with() {
    let dir = crate::paths::scratch_dir("on-n-off-change-cli");
    let cli = CliStub::new("slow").sleep(7).cli(&dir);
    let changes = Changes::at(dir.join("_backups"));
    let backups = changes.io.back_up(AgentId::Codex, &[]).unwrap();
    let step = Step::cli(
        &cli,
        Vec::new(),
        vec![crate::cli::argv(&["plugin", "update", "acme@webapp"])],
    );
    let started = Instant::now();
    let answer = step.run(&changes.io, &backups, Duration::from_millis(200));
    let elapsed = started.elapsed();
    let _ = fs::remove_dir_all(&dir);

    let error = answer.expect_err("the slow step outlived its deadline");
    assert!(
        error.message.to_lowercase().contains("timed out"),
        "{}",
        error.message
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the step returned after {elapsed:?}"
    );
}

#[test]
fn a_change_that_panicked_leaves_the_next_change_free_to_run() {
    let home = Home::new(AgentId::Codex);
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let changes = Changes::faking(home.backups(), move |_, _| {
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("acme step panicked");
        }
        Ok(())
    });
    let request = Request::mcp("github", false);

    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        changes.make(&*home.adapter, request.clone(), None)
    }));
    assert!(first.is_err());

    let answer = changes.make(&*home.adapter, request, None).unwrap();
    let local = crate::drift::read(&*home.adapter, None, crate::drift::Reach::Local).unwrap();
    assert_eq!(answer, local);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
