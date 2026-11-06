use super::super::text::{wrap_px, Typesetter};
use super::*;

#[test]
fn wrapping_fits_words_and_ellipsizes_the_overflow() {
    let lines = wrap_px(
        &FixedAdvance,
        "one two three four five",
        100.0,
        20.0,
        TextWeight::Regular,
        2,
    );
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("one"));
    assert!(
        lines[1].ends_with('…'),
        "the tail is ellipsized: {:?}",
        lines
    );
    for line in &lines {
        assert!(FixedAdvance.measure_px(line, 20.0, TextWeight::Regular) <= 101.0);
    }
    assert_eq!(lines, ["one two", "three fou…"]);
}

#[test]
fn wrapping_handles_empty_and_short_text() {
    let wrap = |text: &str, max_lines: usize| {
        wrap_px(
            &FixedAdvance,
            text,
            100.0,
            20.0,
            TextWeight::Regular,
            max_lines,
        )
    };
    assert!(wrap("", 2).is_empty());
    assert_eq!(wrap("one", 2), ["one"]);
    assert_eq!(wrap("a b", 0).len(), 0);
}

#[test]
fn a_word_wider_than_the_line_keeps_a_line_of_its_own() {
    assert_eq!(
        wrap_px(
            &FixedAdvance,
            "a extraordinarily b",
            100.0,
            20.0,
            TextWeight::Regular,
            3,
        ),
        ["a", "extraordinarily", "b"]
    );
}

#[test]
fn text_is_measured_in_device_pixels_and_answered_in_points() {
    let at_two = Typesetter::new(&FixedAdvance, 2.0);
    assert_eq!(
        at_two.width("abcd", 10.0, TextWeight::Regular),
        20.0,
        "4 characters of 20 device px at half an em, back to points"
    );
    assert_eq!(
        at_two.cap_middle(10.0, TextWeight::Regular),
        (22.0 - 7.0) / 2.0,
        "the cap band is asked at the device size"
    );
    assert_eq!(
        at_two.wrap("one two three", 35.0, 10.0, TextWeight::Regular, 2),
        ["one two", "three"],
        "the wrap width is the line's width in device pixels"
    );
    assert_eq!(
        Typesetter::new(&FixedAdvance, 1.0).wrap("one two", 1.0, 0.0, TextWeight::Regular, 1),
        ["one two"],
        "a size of nothing is not wrapped at all"
    );
}

#[test]
fn an_ellipsized_text_fits_its_width_and_says_it_was_cut() {
    let typeset = Typesetter::new(&FixedAdvance, 1.0);
    assert_eq!(
        typeset.ellipsize("webapp", 60.0, 10.0, TextWeight::Regular),
        "webapp",
        "a text that fits is left alone"
    );
    let cut = typeset.ellipsize("a-long-session-name", 40.0, 10.0, TextWeight::Regular);
    assert_eq!(cut, "a-long-…");
    assert!(typeset.width(&cut, 10.0, TextWeight::Regular) <= 40.0);
    assert_eq!(
        typeset.ellipsize("abc", 1.0, 10.0, TextWeight::Regular),
        "a…",
        "at least one character stays before the ellipsis"
    );
    assert!(
        typeset.width("Open Limits", 11.0, TextWeight::Semibold)
            > typeset.width("Open Limits", 11.0, TextWeight::Regular),
        "the test metrics measure a heavier weight wider, as DirectWrite does"
    );
}
