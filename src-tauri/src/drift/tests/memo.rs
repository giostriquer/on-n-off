use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use super::super::{read, with_fetch_text, Memo, Reach, Transient, REMEMBERED_FOR};
use super::providers::{remote, rows, Home, SUPERPOWERS_MANIFEST, WORKSHOP_MARKETPLACE};
use crate::dto::AgentId;

thread_local! {
    static ASKED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CLOCK: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn ticking() -> Instant {
    CLOCK.with(|clock| {
        let now = clock.get().unwrap_or_else(Instant::now) + Duration::from_millis(1);
        clock.set(Some(now));
        now
    })
}

fn frozen() -> Instant {
    CLOCK.with(|clock| {
        let now = clock.get().unwrap_or_else(Instant::now);
        clock.set(Some(now));
        now
    })
}

fn advance(by: Duration) {
    CLOCK.with(|clock| clock.set(Some(clock.get().unwrap_or_else(Instant::now) + by)));
}

fn memo() -> Memo {
    Memo::on(ticking)
}

fn counting(url: &str) -> Result<Option<String>, Transient> {
    ASKED.with(|asked| asked.borrow_mut().push(url.to_string()));
    remote(url)
}

fn missing(url: &str) -> Result<Option<String>, Transient> {
    ASKED.with(|asked| asked.borrow_mut().push(url.to_string()));
    Ok(None)
}

fn unreachable(url: &str) -> Result<Option<String>, Transient> {
    ASKED.with(|asked| asked.borrow_mut().push(url.to_string()));
    Err(Transient)
}

fn asked_during<T>(run: impl FnOnce() -> T) -> (T, Vec<String>) {
    ASKED.with(|asked| asked.borrow_mut().clear());
    let tab = with_fetch_text(counting, run);
    let mut asked = ASKED.with(|asked| asked.borrow_mut().drain(..).collect::<Vec<_>>());
    asked.sort();
    (tab, asked)
}

fn first_answers() -> Vec<String> {
    let mut expected = vec![
        WORKSHOP_MARKETPLACE.to_string(),
        SUPERPOWERS_MANIFEST.to_string(),
    ];
    expected.sort();
    expected
}

#[test]
fn remote_answers_are_remembered_for_an_hour() {
    assert_eq!(REMEMBERED_FOR, Duration::from_secs(60 * 60));
}

#[test]
fn a_remote_read_reuses_every_answer_younger_than_the_hour() {
    let home = Home::new(AgentId::Claude);
    let memo = memo();
    let (first, asked) = asked_during(|| read(&*home.adapter, None, Reach::Remote(&memo)).unwrap());
    assert_eq!(asked, first_answers());
    advance(REMEMBERED_FOR - Duration::from_secs(1));
    let (again, asked) = asked_during(|| read(&*home.adapter, None, Reach::Remote(&memo)).unwrap());
    assert_eq!(asked, Vec::<String>::new());
    assert_eq!(rows(&again), rows(&first));
}

#[test]
fn an_answer_older_than_its_window_is_read_again() {
    let home = Home::new(AgentId::Claude);
    let memo = memo();
    let (first, _) = asked_during(|| read(&*home.adapter, None, Reach::Remote(&memo)).unwrap());
    advance(REMEMBERED_FOR);
    let (again, asked) = asked_during(|| read(&*home.adapter, None, Reach::Remote(&memo)).unwrap());
    assert_eq!(asked, first_answers());
    assert_eq!(rows(&again), rows(&first));

    let (_, asked) = asked_during(|| {
        memo.reader(false).text(WORKSHOP_MARKETPLACE);
        advance(REMEMBERED_FOR);
        memo.reader(false).text(WORKSHOP_MARKETPLACE);
    });
    assert_eq!(asked, vec![WORKSHOP_MARKETPLACE.to_string()]);
}

#[test]
fn an_answer_is_reused_until_it_is_exactly_an_hour_old() {
    let memo = Memo::on(frozen);
    let (_, asked) = asked_during(|| {
        memo.reader(false).text(WORKSHOP_MARKETPLACE);
        advance(REMEMBERED_FOR - Duration::from_nanos(1));
        memo.reader(false).text(WORKSHOP_MARKETPLACE);
        advance(Duration::from_nanos(1));
        memo.reader(false).text(WORKSHOP_MARKETPLACE);
    });
    assert_eq!(asked, vec![WORKSHOP_MARKETPLACE.to_string(); 2]);
}

#[test]
fn a_forced_read_asks_every_url_again_once() {
    let home = Home::new(AgentId::Claude);
    let memo = memo();
    let (first, _) = asked_during(|| read(&*home.adapter, None, Reach::Remote(&memo)).unwrap());
    let (forced, asked) =
        asked_during(|| read(&*home.adapter, None, Reach::Forced(&memo)).unwrap());
    assert_eq!(asked, first_answers());
    assert_eq!(rows(&forced), rows(&first));
    let (_, asked) = asked_during(|| read(&*home.adapter, None, Reach::Remote(&memo)).unwrap());
    assert_eq!(asked, Vec::<String>::new());

    let reader = memo.reader(true);
    let (_, asked) = asked_during(|| {
        assert!(reader.text(WORKSHOP_MARKETPLACE).is_some());
        assert!(reader.text(WORKSHOP_MARKETPLACE).is_some());
    });
    assert_eq!(asked, vec![WORKSHOP_MARKETPLACE.to_string()]);
}

#[test]
fn a_missing_answer_is_remembered_too() {
    let home = Home::new(AgentId::Claude);
    let memo = memo();
    ASKED.with(|asked| asked.borrow_mut().clear());
    let first = with_fetch_text(missing, || {
        read(&*home.adapter, None, Reach::Remote(&memo)).unwrap()
    });
    assert!(!ASKED.with(|asked| asked.borrow().is_empty()));
    ASKED.with(|asked| asked.borrow_mut().clear());
    let again = with_fetch_text(missing, || {
        read(&*home.adapter, None, Reach::Remote(&memo)).unwrap()
    });
    assert!(ASKED.with(|asked| asked.borrow().is_empty()));
    assert_eq!(rows(&again), rows(&first));
}

#[test]
fn a_failed_request_is_not_remembered_so_the_next_read_asks_again() {
    let home = Home::new(AgentId::Claude);
    let memo = memo();
    ASKED.with(|asked| asked.borrow_mut().clear());
    let offline = with_fetch_text(unreachable, || {
        read(&*home.adapter, None, Reach::Remote(&memo)).unwrap()
    });
    let workbench = |tab: &crate::dto::AgentTabDto| {
        tab.plugins
            .iter()
            .find(|plugin| plugin.id == "workbench@workshop")
            .map(|plugin| plugin.upstream.clone())
    };
    assert_eq!(workbench(&offline).as_deref(), Some("1.1.0"));
    let (online, asked) =
        asked_during(|| read(&*home.adapter, None, Reach::Remote(&memo)).unwrap());
    assert_eq!(asked, first_answers());
    assert_eq!(workbench(&online).as_deref(), Some("2.0.0"));
}

#[test]
fn providers_reading_one_marketplace_share_its_answer() {
    let claude = Home::new(AgentId::Claude);
    let codex = Home::new(AgentId::Codex);
    let memo = memo();
    let _ = asked_during(|| read(&*claude.adapter, None, Reach::Remote(&memo)).unwrap());
    let (tab, asked) = asked_during(|| read(&*codex.adapter, None, Reach::Remote(&memo)).unwrap());
    assert_eq!(asked, Vec::<String>::new());
    let workbench = tab
        .plugins
        .iter()
        .find(|plugin| plugin.id == "workbench@workshop")
        .unwrap();
    assert_eq!(workbench.upstream, "2.0.0");
}
