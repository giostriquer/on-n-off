//! Text on the notch pixmap: native GDI rendering of the system Segoe UI, the same
//! face (and hinting) every Windows app uses, rasterized into a coverage buffer and
//! blended into the premultiplied pixmap. This is the Windows counterpart of the
//! macOS helper's system font.
//!
//! GDI needs an unsafe block per call, which is why this module opts in explicitly.

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tiny_skia::Pixmap;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC,
    GetTextMetricsW, ReleaseDC, SelectObject, SetBkMode, SetMapMode, SetTextColor,
    ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CALCRECT, DT_NOPREFIX, DT_SINGLELINE,
    FF_DONTCARE, FONT_WEIGHT, FW_NORMAL, FW_SEMIBOLD, MM_TEXT, OUT_OUTLINE_PRECIS, TEXTMETRICW,
    TRANSPARENT,
};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Weight {
    Regular,
    Semibold,
}

fn weight_of(weight: Weight) -> FONT_WEIGHT {
    match weight {
        Weight::Regular => FW_NORMAL,
        Weight::Semibold => FW_SEMIBOLD,
    }
}

/// Cached native font handles, keyed by (pixel height, weight). Handles live for the
/// process; only a handful of sizes are ever requested.
struct FontCache {
    fonts: HashMap<(i32, u8), isize>,
}
static FONT_CACHE: OnceLock<Mutex<FontCache>> = OnceLock::new();

fn font_cache() -> &'static Mutex<FontCache> {
    FONT_CACHE.get_or_init(|| {
        Mutex::new(FontCache {
            fonts: HashMap::new(),
        })
    })
}

unsafe fn create_font(px: i32, weight: Weight) -> isize {
    let key = (
        px,
        match weight {
            Weight::Regular => 0u8,
            Weight::Semibold => 1u8,
        },
    );
    let cache = font_cache();
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(handle) = cache.fonts.get(&key) {
        return *handle;
    }
    let mut face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    let handle = unsafe {
        CreateFontW(
            -px,
            0,
            0,
            0,
            weight_of(weight).0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_OUTLINE_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            windows::core::PWSTR(face.as_mut_ptr()),
        )
    };
    cache.fonts.insert(key, handle.0 as isize);
    handle.0 as isize
}

/// A text string rasterized by GDI into a single-channel coverage buffer.
struct Raster {
    coverage: Vec<u8>,
    width: i32,
    height: i32,
    ascent: i32,
}

/// Renders `text` white-on-black at `px` height with native hinting; the red channel
/// of the result is the per-pixel coverage.
unsafe fn rasterize(text: &str, px: i32, weight: Weight) -> Raster {
    let screen = unsafe { GetDC(None) };
    let dc = unsafe { CreateCompatibleDC(Some(screen)) };
    let hfont = unsafe { create_font(px, weight) };
    let old_font =
        unsafe { SelectObject(dc, windows::Win32::Graphics::Gdi::HGDIOBJ(hfont as *mut _)) };
    unsafe { SetMapMode(dc, MM_TEXT) };
    let mut metrics = TEXTMETRICW::default();
    let _ = unsafe { GetTextMetricsW(dc, &mut metrics) };
    let mut wide: Vec<u16> = text.encode_utf16().collect();

    let mut measure = RECT::default();
    unsafe {
        DrawTextW(
            dc,
            &mut wide,
            &mut measure,
            DT_CALCRECT | DT_NOPREFIX | DT_SINGLELINE,
        )
    };
    let width = (measure.right - measure.left).max(1);
    let height = (measure.bottom - measure.top).max(1);

    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let raster = unsafe { CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0) };
    if let Ok(hbmp) = raster {
        let old_bmp = unsafe { SelectObject(dc, hbmp.into()) };
        unsafe {
            std::ptr::write_bytes(bits as *mut u8, 0, (width * height * 4) as usize);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0x00ff_ffff));
            let mut box_ = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            DrawTextW(dc, &mut wide, &mut box_, DT_NOPREFIX | DT_SINGLELINE);
        }
        let len = (width * height) as usize;
        let mut coverage = Vec::with_capacity(len);
        for index in 0..len {
            coverage.push(unsafe { *(bits as *const u8).add(index * 4) });
        }
        unsafe { SelectObject(dc, old_bmp) };
        unsafe {
            let _ = DeleteObject(hbmp.into());
        }
        unsafe { SelectObject(dc, old_font) };
        unsafe {
            let _ = DeleteDC(dc);
        };
        unsafe {
            let _ = ReleaseDC(None, screen);
        };
        Raster {
            coverage,
            width,
            height,
            ascent: metrics.tmAscent,
        }
    } else {
        unsafe { SelectObject(dc, old_font) };
        unsafe {
            let _ = DeleteDC(dc);
        };
        unsafe {
            let _ = ReleaseDC(None, screen);
        };
        Raster {
            coverage: Vec::new(),
            width: 1,
            height: 1,
            ascent: px,
        }
    }
}

/// Distance from a line's top to its baseline, in device pixels at `size`.
pub(super) fn ascent_px(size: f32, weight: Weight) -> f32 {
    let px = size.round().max(1.0) as i32;
    let screen = unsafe { GetDC(None) };
    let dc = unsafe { CreateCompatibleDC(Some(screen)) };
    let hfont = unsafe { create_font(px, weight) };
    let old = unsafe { SelectObject(dc, windows::Win32::Graphics::Gdi::HGDIOBJ(hfont as *mut _)) };
    let mut metrics = TEXTMETRICW::default();
    let _ = unsafe { GetTextMetricsW(dc, &mut metrics) };
    unsafe { SelectObject(dc, old) };
    unsafe {
        let _ = DeleteDC(dc);
    };
    unsafe {
        let _ = ReleaseDC(None, screen);
    };
    metrics.tmAscent as f32
}

/// Advance width of `text` in device pixels when drawn at `size`.
pub(super) fn measure_px(text: &str, size: f32, weight: Weight) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let px = size.round().max(1.0) as i32;
    let screen = unsafe { GetDC(None) };
    let dc = unsafe { CreateCompatibleDC(Some(screen)) };
    let hfont = unsafe { create_font(px, weight) };
    let old = unsafe { SelectObject(dc, windows::Win32::Graphics::Gdi::HGDIOBJ(hfont as *mut _)) };
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rect = RECT::default();
    unsafe {
        DrawTextW(
            dc,
            &mut wide,
            &mut rect,
            DT_CALCRECT | DT_NOPREFIX | DT_SINGLELINE,
        )
    };
    unsafe { SelectObject(dc, old) };
    unsafe {
        let _ = DeleteDC(dc);
    };
    unsafe {
        let _ = ReleaseDC(None, screen);
    };
    (rect.right - rect.left).max(0) as f32
}

/// Draws `text` with its left edge at `x` and its baseline at `baseline`, in device pixels.
pub(super) fn draw_px(
    pixmap: &mut Pixmap,
    x: f32,
    baseline: f32,
    text: &str,
    size: f32,
    weight: Weight,
    color: [u8; 4],
) {
    if text.is_empty() {
        return;
    }
    let px = size.round().max(1.0) as i32;
    let raster = unsafe { rasterize(text, px, weight) };
    if raster.coverage.is_empty() {
        return;
    }
    let left = x.round();
    let top = baseline - raster.ascent as f32;
    blend(
        pixmap,
        &raster.coverage,
        raster.width as usize,
        raster.height as usize,
        left,
        top,
        color,
    );
}

/// Draws `text` flush to the right edge of the box starting at `x`, `width` wide.
pub(super) fn draw_px_right(
    pixmap: &mut Pixmap,
    x: f32,
    width: f32,
    baseline: f32,
    text: &str,
    size: f32,
    weight: Weight,
    color: [u8; 4],
) {
    let measured = measure_px(text, size, weight);
    draw_px(
        pixmap,
        x + width - measured,
        baseline,
        text,
        size,
        weight,
        color,
    );
}

/// Word-wraps `text` into at most `max_lines` lines no wider than `max_px`, with an
/// ellipsis on the last line when a word was cut.
pub(super) fn wrap_px(
    text: &str,
    max_px: f32,
    size: f32,
    weight: Weight,
    max_lines: usize,
) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }
    let mut words = text.split_whitespace().peekable();
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    while let Some(word) = words.peek() {
        let candidate = if current.is_empty() {
            (*word).to_string()
        } else {
            format!("{current} {word}")
        };
        if measure_px(&candidate, size, weight) <= max_px || current.is_empty() {
            current = candidate;
            words.next();
        } else {
            lines.push(std::mem::take(&mut current));
            if lines.len() == max_lines {
                break;
            }
        }
    }
    if !current.is_empty() || (lines.is_empty() && !text.trim().is_empty()) {
        lines.push(current);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
    }
    if words.peek().is_some() {
        if let Some(last) = lines.last_mut() {
            let mut cut = last.trim_end().to_string();
            while !cut.is_empty() && measure_px(&format!("{cut}…"), size, weight) > max_px {
                cut.pop();
            }
            *last = format!("{cut}…");
        }
    }
    lines
}

/// Coverage-alpha blending of a rasterized text buffer into a premultiplied pixmap.
fn blend(
    pixmap: &mut Pixmap,
    bitmap: &[u8],
    width: usize,
    height: usize,
    x: f32,
    y: f32,
    color: [u8; 4],
) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    let left = x.round() as i32;
    let top = y.round() as i32;
    for row in 0..height {
        let py = top + row as i32;
        if py < 0 || py >= ph {
            continue;
        }
        for col in 0..width {
            let px = left + col as i32;
            if px < 0 || px >= pw {
                continue;
            }
            let coverage = bitmap[row * width + col];
            if coverage == 0 {
                continue;
            }
            let index = (py * pw + px) as usize;
            let dst = pixmap.data_mut();
            let d = (
                u32::from(dst[index * 4]),
                u32::from(dst[index * 4 + 1]),
                u32::from(dst[index * 4 + 2]),
                u32::from(dst[index * 4 + 3]),
            );
            // Straight coverage over the source colour, source-over onto premultiplied dst.
            let (sr, sg, sb, sa) = (
                u32::from(color[0]),
                u32::from(color[1]),
                u32::from(color[2]),
                u32::from(color[3]),
            );
            let a = u32::from(coverage) * sa / 255;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            let out_a = a + d.3 * inv / 255;
            let premul =
                |channel: u32, dst_channel: u32| channel * a / 255 + dst_channel * inv / 255;
            dst[index * 4] = premul(sr, d.0).min(out_a) as u8;
            dst[index * 4 + 1] = premul(sg, d.1).min(out_a) as u8;
            dst[index * 4 + 2] = premul(sb, d.2).min(out_a) as u8;
            dst[index * 4 + 3] = out_a.min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
