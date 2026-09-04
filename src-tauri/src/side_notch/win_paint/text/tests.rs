use super::*;

#[test]
fn the_face_is_one_the_system_really_has() {
    let family = with_engine(|engine| Some(engine.family)).expect("DirectWrite starts");
    assert!(
        family == NOTCH_FACE || family == FALLBACK_FACE,
        "the notch draws in a face it asked for: {family}"
    );
    // And every weight it draws resolves to a face of its own.
    for weight in [Weight::Regular, Weight::Medium, Weight::Semibold] {
        let width = measure_px("Open Limits", 11.0, weight);
        assert!(width > 0.0, "the {family} face renders at every weight");
    }
    let regular = measure_px("Open Limits", 11.0, Weight::Regular);
    let heavy = measure_px("Open Limits", 11.0, Weight::Semibold);
    assert!(
        heavy > regular,
        "semibold is the heavier face: {regular} vs {heavy}"
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
        Weight::Semibold,
        [255, 255, 255, 255],
    );
    pixmap.pixels().iter().map(|px| px.alpha()).collect()
}

#[test]
fn glyphs_carry_more_greys_than_gdi_smoothing() {
    // `ANTIALIASED_QUALITY`, the GDI path this replaced, is a 4x4 supersample: exactly
    // 16 coverage levels, and curves that stair-step beside the app's own text.
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
    // An alpha texture is linear coverage. Every Windows text stack — the app's own
    // WebView included — corrects it before blending; without that, light text on a
    // dark panel comes out thin and washed out next to the app. The curve was fitted
    // against that engine, so it has to be a real correction, not the identity.
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
fn wrapping_fits_words_and_ellipsizes_the_overflow() {
    let lines = wrap_px("one two three four five", 100.0, 20.0, Weight::Regular, 2);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("one"));
    assert!(
        lines[1].ends_with('…'),
        "the tail is ellipsized: {:?}",
        lines
    );
    for line in &lines {
        assert!(measure_px(line, 20.0, Weight::Regular) <= 101.0);
    }
}

#[test]
fn wrapping_handles_empty_and_short_text() {
    assert!(wrap_px("", 100.0, 20.0, Weight::Regular, 2).is_empty());
    assert_eq!(wrap_px("one", 100.0, 20.0, Weight::Regular, 2), ["one"]);
    assert_eq!(wrap_px("a b", 100.0, 20.0, Weight::Regular, 0).len(), 0);
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
        Weight::Semibold,
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
    let measured = measure_px(text, size, Weight::Semibold);
    let baseline = 40.0;
    draw_px(
        &mut pixmap,
        10.0,
        baseline,
        text,
        size,
        Weight::Semibold,
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
    // `Segoe UI` ships 300/350/400/600/700 and no 500. CSS would step a 500 request
    // down onto regular; DirectWrite steps it up onto semibold, and DirectWrite is what
    // resolves the app's own `font-medium` runs inside the WebView. Scoring the family
    // here instead put the overlay's percentages a whole weight lighter than the same
    // figures in the app.
    let sample = "Open Limits 81% Used";
    let regular = ink(sample, 17.0, Weight::Regular);
    let medium = ink(sample, 17.0, Weight::Medium);
    let semibold = ink(sample, 17.0, Weight::Semibold);
    assert!(
        medium > regular,
        "medium is not the regular cut: regular {regular}, medium {medium}"
    );
    assert_eq!(
        medium, semibold,
        "it is the cut DirectWrite hands the WebView for a 500"
    );
}

/// Total coverage a run puts on the pixmap, as a stand-in for how heavy it reads.
fn ink(text: &str, size: f32, weight: Weight) -> u64 {
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
    // The mac header is an `HStack`, and the app's own rows are `flex items-center`;
    // on both, a mark beside a title lands on the middle of the capitals, not on the
    // middle of the ink. Segoe UI's descender is deep enough that centring on the ink
    // instead drops the mark a visible step below the title it belongs to.
    let (size, weight, baseline) = (13.0f32, Weight::Semibold, 30.0f32);
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

    let placed = baseline - ascent_px(size, weight) + cap_middle_px(size, weight);
    assert!(
        (placed - cap_centre).abs() <= 1.0,
        "mark centre {placed} sits on the cap band centre {cap_centre} (cap top {cap_top})"
    );
}
