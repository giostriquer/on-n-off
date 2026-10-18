use super::*;
use crate::cli_stub::CliStub;
use std::cell::{Cell, RefCell};
use std::process::Stdio;

fn spawn(stub: CliStub, name: &str) -> Child {
    stub.cli(&crate::paths::scratch_dir(name))
        .command()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
fn a_line_is_seen_while_the_command_still_runs() {
    let child = spawn(
        CliStub::new("prints-then-waits").stdout("ready").sleep(30),
        "process-line-while-running",
    );
    let seen = RefCell::new(Vec::new());
    let started = Instant::now();

    let outcome = wait_with_cancellation(
        child,
        Duration::from_secs(20),
        || !seen.borrow().is_empty(),
        |line| seen.borrow_mut().push(line.to_owned()),
    )
    .unwrap();

    assert!(matches!(outcome, CommandOutcome::TimedOut));
    assert_eq!(*seen.borrow(), ["ready"]);
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn every_line_is_seen_in_order_and_kept_in_the_output() {
    let child = spawn(
        CliStub::new("prints-two-lines")
            .stdout("first")
            .stderr("noise"),
        "process-lines-in-order",
    );
    let seen = RefCell::new(Vec::new());

    let outcome = wait_with_cancellation(
        child,
        Duration::from_secs(20),
        || false,
        |line| seen.borrow_mut().push(line.to_owned()),
    )
    .unwrap();

    let CommandOutcome::Exited {
        success,
        stdout,
        stderr,
    } = outcome
    else {
        panic!("the command exited");
    };
    assert!(success);
    assert_eq!(*seen.borrow(), ["first"]);
    assert_eq!(stdout.trim_end(), "first");
    assert_eq!(stderr.trim_end(), "noise");
}

#[test]
fn a_canceled_command_reports_no_further_lines() {
    let child = spawn(
        CliStub::new("prints-then-sleeps").stdout("unread").sleep(2),
        "process-canceled-lines",
    );
    let seen = Cell::new(0);

    let outcome = wait_with_cancellation(
        child,
        Duration::from_secs(20),
        || true,
        |_| seen.set(seen.get() + 1),
    )
    .unwrap();

    assert!(matches!(outcome, CommandOutcome::TimedOut));
    assert_eq!(seen.get(), 0);
}
