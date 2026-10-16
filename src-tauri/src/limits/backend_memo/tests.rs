use super::*;
use std::cell::Cell;

const INTERVAL: Duration = Duration::from_secs(60);

impl<T: Clone> PerAccount<T> {
    fn failed_before(&self, account: &str, count: u32) {
        let expired = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        self.entries()
            .entry(account.to_string())
            .or_default()
            .failure = Some((expired, count));
    }

    fn backoff_of(&self, account: &str) -> Option<(Instant, u32)> {
        self.entries().get(account).and_then(|entry| entry.failure)
    }
}

fn reads(answer: Option<u8>) -> (impl Fn() -> Option<u8>, std::rc::Rc<Cell<u32>>) {
    let asked = std::rc::Rc::new(Cell::new(0));
    let counter = asked.clone();
    (
        move || {
            counter.set(counter.get() + 1);
            answer
        },
        asked,
    )
}

#[test]
fn an_answer_stands_for_its_time_and_is_asked_again_after() {
    let memo: PerAccount<u8> = PerAccount::new(Some(Duration::from_millis(80)));
    let (read, asked) = reads(Some(7));
    assert_eq!(memo.read_backed_off("a", INTERVAL, &read), Some(7));
    assert_eq!(memo.read_backed_off("a", INTERVAL, &read), Some(7));
    assert_eq!(asked.get(), 1, "the standing answer is served, not re-read");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(memo.read_backed_off("a", INTERVAL, &read), Some(7));
    assert_eq!(asked.get(), 2, "an answer past its time is read again");
}

#[test]
fn without_a_standing_time_every_read_asks() {
    let memo: PerAccount<u8> = PerAccount::new(None);
    let (read, asked) = reads(Some(1));
    assert_eq!(memo.read_backed_off("a", INTERVAL, &read), Some(1));
    assert_eq!(memo.read_backed_off("a", INTERVAL, &read), Some(1));
    assert_eq!(asked.get(), 2);
    assert_eq!(memo.backoff_of("a"), None);
}

#[test]
fn a_failure_holds_the_account_back_doubling_each_time_and_a_success_clears_it() {
    let memo: PerAccount<u8> = PerAccount::new(None);
    let (fail, asked) = reads(None);
    assert_eq!(memo.read_backed_off("a", INTERVAL, &fail), None);
    assert_eq!(memo.read_backed_off("a", INTERVAL, &fail), None);
    assert_eq!(asked.get(), 1, "held back within the wait");
    let (_, count) = memo.backoff_of("a").unwrap();
    assert_eq!(count, 1);

    memo.failed_before("a", 2);
    let before = Instant::now();
    assert_eq!(memo.read_backed_off("a", INTERVAL, &fail), None);
    let after = Instant::now();
    let (until, count) = memo.backoff_of("a").unwrap();
    assert_eq!(count, 3, "another failure once the wait ran out counts on");
    let wait = INTERVAL * 4;
    assert!(
        until >= before + wait && until <= after + wait,
        "and waits the longer delay from then"
    );

    let (read, _) = reads(Some(9));
    assert_eq!(memo.read_backed_off("b", INTERVAL, &read), Some(9));

    memo.failed_before("a", 3);
    assert_eq!(memo.read_backed_off("a", INTERVAL, &read), Some(9));
    assert_eq!(memo.backoff_of("a"), None, "a success clears the backoff");
    memo.forget("a");
    assert_eq!(memo.backoff_of("a"), None);
}

#[test]
fn a_backend_figure_that_keeps_failing_waits_one_two_four_then_sixteen_intervals() {
    for interval in [INTERVAL, Duration::from_secs(300)] {
        for (failures, intervals) in [(1, 1), (2, 2), (3, 4), (5, 16)] {
            let memo: PerAccount<u8> = PerAccount::new(None);
            if failures > 1 {
                memo.failed_before("a", failures - 1);
            }
            let (fail, _) = reads(None);
            let start = Instant::now();
            assert_eq!(memo.read_backed_off("a", interval, &fail), None);
            let end = Instant::now();
            let (until, count) = memo.backoff_of("a").unwrap();
            let wait = (interval * intervals).min(Duration::from_secs(3600));
            assert_eq!(count, failures);
            assert!(
                until >= start + wait && until <= end + wait,
                "{failures} failures at {interval:?} wait {wait:?}"
            );
        }
    }
}
