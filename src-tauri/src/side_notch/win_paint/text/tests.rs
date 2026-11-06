use super::*;

#[test]
fn the_face_is_one_the_system_really_has() {
    let family = with_engine(|engine| Some(engine.family)).expect("DirectWrite starts");
    assert!(
        family == NOTCH_FACE || family == FALLBACK_FACE,
        "the notch draws in a face it asked for: {family}"
    );
    for weight in [
        TextWeight::Regular,
        TextWeight::Medium,
        TextWeight::Semibold,
    ] {
        let width = DirectWrite.measure_px("Open Limits", 11.0, weight);
        assert!(width > 0.0, "the {family} face renders at every weight");
    }
    let regular = DirectWrite.measure_px("Open Limits", 11.0, TextWeight::Regular);
    let heavy = DirectWrite.measure_px("Open Limits", 11.0, TextWeight::Semibold);
    assert!(
        heavy > regular,
        "semibold is the heavier face: {regular} vs {heavy}"
    );
    let medium = DirectWrite.measure_px("Open Limits", 11.0, TextWeight::Medium);
    assert!(
        medium > regular,
        "a medium run is heavier than a regular one: {regular} vs {medium}"
    );
}

fn drawn(x: f32) -> Vec<u8> {
    let mut pixmap = Pixmap::new(200, 40).unwrap();
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    draw_px(
        &mut pixmap,
        x,
        28.0,
        "Claude Usage 81%",
        13.0,
        TextWeight::Semibold,
        [255, 255, 255, 255],
    );
    pixmap.pixels().iter().map(|px| px.alpha()).collect()
}

#[test]
fn glyphs_carry_more_greys_than_gdi_smoothing() {
    let mut levels = drawn(4.0);
    levels.sort_unstable();
    levels.dedup();
    assert!(
        levels.len() > 16,
        "more than plain grey antialiasing: {} levels",
        levels.len()
    );
}

#[test]
fn coverage_is_gamma_corrected_like_the_app_engine() {
    let table = with_engine(|engine| Some(engine.gamma)).expect("DirectWrite starts");
    assert_eq!(table[0], 0, "nothing stays nothing");
    assert_eq!(table[255], 255, "and full coverage stays full");
    assert!(
        (185..=200).contains(&table[128]),
        "a half-covered pixel blends the way the WebView blends it: {}",
        table[128]
    );
    assert!(
        table.windows(2).all(|pair| pair[0] <= pair[1]),
        "the curve only ever rises"
    );
}

#[test]
fn drawing_land_pixels_without_touching_the_rest() {
    let mut pixmap = Pixmap::new(120, 40).unwrap();
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    draw_px(
        &mut pixmap,
        10.0,
        30.0,
        "42%",
        17.0,
        TextWeight::Semibold,
        [255, 255, 255, 255],
    );
    let mut lit = 0;
    for pixel in pixmap.pixels() {
        if pixel.alpha() > 0 {
            lit += 1;
        }
    }
    assert!(lit > 30, "the digits draw: {lit}");
    assert_eq!(pixmap.pixel(0, 0).unwrap().alpha(), 0, "outside the text");
}

#[test]
fn measure_matches_draw_width() {
    let mut pixmap = Pixmap::new(200, 60).unwrap();
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    let text = "Open Limits";
    let size = 13.0;
    let measured = DirectWrite.measure_px(text, size, TextWeight::Semibold);
    let baseline = 40.0;
    draw_px(
        &mut pixmap,
        10.0,
        baseline,
        text,
        size,
        TextWeight::Semibold,
        [255, 255, 255, 255],
    );
    let mut left = f32::MAX;
    let mut right = f32::MIN;
    for (index, pixel) in pixmap.pixels().iter().enumerate() {
        if pixel.alpha() > 0 {
            let x = (index % 200) as f32;
            left = left.min(x);
            right = right.max(x);
        }
    }
    assert!(right.is_finite(), "something drew");
    assert!(
        (right - left) - measured <= 3.0,
        "drawn width {} matches measure {}",
        right - left,
        measured
    );
}

#[test]
fn medium_runs_land_on_the_face_the_webview_would_pick() {
    let sample = "Open Limits 81% Used";
    let regular = ink(sample, 17.0, TextWeight::Regular);
    let medium = ink(sample, 17.0, TextWeight::Medium);
    let semibold = ink(sample, 17.0, TextWeight::Semibold);
    assert!(
        medium > regular,
        "medium is not the regular cut: regular {regular}, medium {medium}"
    );
    assert_eq!(
        medium, semibold,
        "it is the cut DirectWrite hands the WebView for a 500"
    );
}

fn ink(text: &str, size: f32, weight: TextWeight) -> u64 {
    let mut pixmap = Pixmap::new(400, 60).unwrap();
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    draw_px(
        &mut pixmap,
        8.0,
        40.0,
        text,
        size,
        weight,
        [255, 255, 255, 255],
    );
    pixmap.pixels().iter().map(|px| u64::from(px.alpha())).sum()
}

#[test]
fn a_glyph_beside_a_title_centres_on_its_cap_band() {
    let (size, weight, baseline) = (13.0f32, TextWeight::Semibold, 30.0f32);
    let mut pixmap = Pixmap::new(120, 48).unwrap();
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    draw_px(
        &mut pixmap,
        8.0,
        baseline,
        "HH",
        size,
        weight,
        [255, 255, 255, 255],
    );
    let cap_top = (0..48)
        .find(|row| (0..120).any(|col| pixmap.pixels()[row * 120 + col].alpha() > 40))
        .expect("the capitals land on the pixmap") as f32;
    let cap_centre = (cap_top + baseline) / 2.0;

    let placed = baseline - ascent_px(size, weight) + DirectWrite.cap_middle_px(size, weight);
    assert!(
        (placed - cap_centre).abs() <= 1.0,
        "mark centre {placed} sits on the cap band centre {cap_centre} (cap top {cap_top})"
    );
}

#[test]
fn a_right_aligned_run_ends_on_the_right_edge_of_its_box() {
    let (x, width, text, size, weight) = (24.0f32, 150.0f32, "81%", 13.0f32, TextWeight::Medium);
    let mut pixmap = Pixmap::new(200, 40).unwrap();
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    draw_px_right(
        &mut pixmap,
        x,
        width,
        28.0,
        text,
        size,
        weight,
        [255, 255, 255, 255],
    );
    let measured = DirectWrite.measure_px(text, size, weight);
    assert!(measured > 0.0, "DirectWrite measures the run");
    let inked: Vec<f32> = pixmap
        .pixels()
        .iter()
        .enumerate()
        .filter(|(_, pixel)| pixel.alpha() > 0)
        .map(|(index, _)| (index % 200) as f32)
        .collect();
    assert!(!inked.is_empty(), "the run draws");
    let left = inked.iter().copied().fold(f32::MAX, f32::min);
    let right = inked.iter().copied().fold(f32::MIN, f32::max);
    let edge = x + width;
    assert!(
        (right - edge).abs() <= 2.0,
        "the ink ends on the box's right edge {edge}: {right}"
    );
    assert!(
        left >= edge - measured - 2.0,
        "nothing lands left of where a {measured} px run starts: {left}"
    );
}

fn without_direct_write<R: Send + 'static>(read: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::spawn(move || {
        ENGINE.with(|cell| *cell.borrow_mut() = Some(None));
        assert!(
            with_engine(|_| Some(())).is_none(),
            "this thread has no DirectWrite"
        );
        read()
    })
    .join()
    .expect("the thread without DirectWrite answers")
}

#[test]
fn without_direct_write_a_mark_still_centres_on_a_cap_band_of_seven_tenths_of_the_size() {
    for size in [10.0f32, 13.0, 17.0, 26.0] {
        let (ascent, cap_middle) = without_direct_write(move || {
            (
                ascent_px(size, TextWeight::Semibold),
                DirectWrite.cap_middle_px(size, TextWeight::Semibold),
            )
        });
        assert!(
            cap_middle > 0.0 && cap_middle < ascent,
            "at {size} px the cap middle {cap_middle} lies inside the ascent {ascent}"
        );
        let above_baseline = ascent - cap_middle;
        assert!(
            (above_baseline - 0.35 * size).abs() <= 0.5,
            "at {size} px a mark centres {above_baseline} px above the baseline, \
             half a 0.7 em cap band give or take the ascent's rounding"
        );
    }
}
