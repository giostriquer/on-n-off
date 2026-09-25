use super::*;
use std::cell::Cell;

/// A read that answers what the test says and counts how often it was asked.
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
    assert_eq!(memo.read_backed_off("a", &read), Some(7));
    assert_eq!(memo.read_backed_off("a", &read), Some(7));
    assert_eq!(asked.get(), 1, "the standing answer is served, not re-read");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(memo.read_backed_off("a", &read), Some(7));
    assert_eq!(asked.get(), 2, "an answer past its time is read again");
}

#[test]
fn without_a_standing_time_every_read_asks() {
    let memo: PerAccount<u8> = PerAccount::new(None);
    let (read, asked) = reads(Some(1));
    assert_eq!(memo.read_backed_off("a", &read), Some(1));
    assert_eq!(memo.read_backed_off("a", &read), Some(1));
    assert_eq!(asked.get(), 2);
    assert_eq!(memo.backoff_of("a"), None);
}

#[test]
fn a_failure_holds_the_account_back_doubling_each_time_and_a_success_clears_it() {
    let memo: PerAccount<u8> = PerAccount::new(None);
    let (fail, asked) = reads(None);
    assert_eq!(memo.read_backed_off("a", &fail), None);
    assert_eq!(memo.read_backed_off("a", &fail), None);
    assert_eq!(asked.get(), 1, "held back within the wait");
    let (_, count) = memo.backoff_of("a").unwrap();
    assert_eq!(count, 1);

    memo.failed_before("a", 2);
    assert_eq!(memo.read_backed_off("a", &fail), None);
    assert_eq!(
        memo.backoff_of("a").unwrap().1,
        3,
        "another failure once the wait ran out counts on"
    );

    // Another account is not held back by this one's failures.
    let (read, _) = reads(Some(9));
    assert_eq!(memo.read_backed_off("b", &read), Some(9));

    memo.failed_before("a", 3);
    assert_eq!(memo.read_backed_off("a", &read), Some(9));
    assert_eq!(memo.backoff_of("a"), None, "a success clears the backoff");
    memo.forget("a");
    assert_eq!(memo.backoff_of("a"), None);
}

#[test]
fn each_failure_in_a_row_doubles_the_wait_up_to_sixteen_intervals_and_an_hour() {
    let minute = Duration::from_secs(60);
    for (count, intervals) in [(0, 1), (1, 1), (2, 2), (3, 4), (5, 16), (9, 16)] {
        assert_eq!(backoff_delay(count, minute), minute * intervals, "{count}");
    }
    assert_eq!(
        backoff_delay(1, Duration::from_secs(7200)),
        Duration::from_secs(3600)
    );
    assert_eq!(
        backoff_delay(3, Duration::from_secs(1000)),
        Duration::from_secs(3600)
    );
}
