//! Text on the notch pixmap: DirectWrite shapes and rasterises the system UI face,
//! the same engine the app's WebView draws with, so the overlay's labels match the
//! rest of on-n-off instead of the coarser 16-level grey GDI produces. Glyph runs go
//! through `IDWriteGlyphRunAnalysis`, whose ClearType texture is averaged back into a
//! single coverage value: a layered window composites per-pixel alpha and cannot show
//! subpixel colour.
//!
//! DirectWrite is COM, which is why this module opts into unsafe explicitly.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use tiny_skia::Pixmap;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_TEXTURE_ALIASED_1x1, DWRITE_TEXTURE_CLEARTYPE_3x1, DWriteCreateFactory, IDWriteFactory,
    IDWriteFactory2, IDWriteFont, IDWriteFontCollection, IDWriteFontFace, IDWriteGlyphRunAnalysis,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_METRICS, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_GLYPH_METRICS,
    DWRITE_GLYPH_RUN, DWRITE_GRID_FIT_MODE_ENABLED, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
    DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Weight {
    Regular,
    /// The mac design's `.medium`. `Segoe UI Variable` carries a real 500, so these
    /// runs stay a step below semibold instead of collapsing onto it.
    Medium,
    Semibold,
}

impl Weight {
    fn dwrite(self) -> DWRITE_FONT_WEIGHT {
        match self {
            Self::Regular => DWRITE_FONT_WEIGHT_NORMAL,
            Self::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Self::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        }
    }
}

/// The face the rest of on-n-off renders in. `ui/src/tokens.css` asks for
/// `-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui,
/// sans-serif`; on Windows the first three do not exist, so the WebView draws in
/// `Segoe UI` and so does the notch. `Tahoma` is the last resort, on every Windows.
const NOTCH_FACE: &str = "Segoe UI";
const FALLBACK_FACE: &str = "Tahoma";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// One face at one weight, with the metrics needed to place and measure it.
struct Face {
    face: IDWriteFontFace,
    units_per_em: f32,
    ascent: f32,
    descent: f32,
    cap_height: f32,
}

impl Face {
    /// Design units to pixels at `size`.
    fn scale(&self, size: f32) -> f32 {
        size / self.units_per_em
    }
}

/// The DirectWrite objects, one set per thread. They are only ever touched from the
/// notch's own window thread (and from tests), so thread-local storage keeps the COM
/// pointers off any shared state.
struct Engine {
    factory: IDWriteFactory,
    collection: IDWriteFontCollection,
    family: &'static str,
    faces: HashMap<Weight, Face>,
    /// Coverage -> blended coverage, under the system's text gamma. An alpha texture
    /// is linear coverage; every Windows text stack (the app's WebView included)
    /// gamma-corrects it before blending, and skipping that is what leaves light text
    /// on a dark panel looking thin and washed out.
    gamma: [u8; 256],
}

thread_local! {
    static ENGINE: RefCell<Option<Option<Engine>>> = const { RefCell::new(None) };
}

impl Engine {
    fn new() -> Option<Self> {
        // SAFETY: DirectWrite creation; every handle is owned by the returned value.
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let mut collection = None;
            factory
                .GetSystemFontCollection(&mut collection, false)
                .ok()?;
            let collection = collection?;
            let family = if has_family(&collection, NOTCH_FACE) {
                NOTCH_FACE
            } else {
                FALLBACK_FACE
            };
            let gamma = gamma_table(&factory);
            Some(Self {
                factory,
                collection,
                family,
                faces: HashMap::new(),
                gamma,
            })
        }
    }

    fn face(&mut self, weight: Weight) -> Option<&Face> {
        if !self.faces.contains_key(&weight) {
            let face = self.load(weight)?;
            self.faces.insert(weight, face);
        }
        self.faces.get(&weight)
    }

    fn load(&self, weight: Weight) -> Option<Face> {
        let face = self.installed_face(weight)?;
        // SAFETY: reading metrics off a face this call owns.
        let mut metrics = DWRITE_FONT_METRICS::default();
        unsafe { face.GetMetrics(&mut metrics) };
        Some(Face {
            face,
            units_per_em: f32::from(metrics.designUnitsPerEm.max(1)),
            ascent: f32::from(metrics.ascent),
            descent: f32::from(metrics.descent),
            cap_height: f32::from(metrics.capHeight),
        })
    }

    /// The installed instance closest to this weight, ties going to the lighter one.
    /// `Segoe UI` ships 300/350/400/600/700 and no 500, and DirectWrite's own matching
    /// rounds a request for 500 *up* onto semibold — a full step heavier than the mac
    /// design's `.medium` runs, and the thing that reads as chunky at these sizes.
    fn installed_face(&self, weight: Weight) -> Option<IDWriteFontFace> {
        // SAFETY: a family lookup and the font face it hands back.
        unsafe {
            let name = wide(self.family);
            let mut index = 0u32;
            let mut exists = windows::core::BOOL(0);
            self.collection
                .FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists)
                .ok()?;
            if !exists.as_bool() {
                return None;
            }
            let family = self.collection.GetFontFamily(index).ok()?;
            let wanted = weight.dwrite().0;
            let mut best: Option<(i32, IDWriteFont)> = None;
            for slot in 0..family.GetFontCount() {
                let Ok(font) = family.GetFont(slot) else {
                    continue;
                };
                if font.GetStyle() != DWRITE_FONT_STYLE_NORMAL
                    || font.GetStretch() != DWRITE_FONT_STRETCH_NORMAL
                {
                    continue;
                }
                let have = font.GetWeight().0;
                let distance = (have - wanted).abs() * 2 + i32::from(have > wanted);
                if best.as_ref().is_none_or(|(closest, _)| distance < *closest) {
                    best = Some((distance, font));
                }
            }
            best?.1.CreateFontFace().ok()
        }
    }
}

/// The system's text gamma as a lookup table over coverage.
fn gamma_table(factory: &IDWriteFactory) -> [u8; 256] {
    // SAFETY: reads two scalars off a rendering-params object owned here.
    let gamma = unsafe {
        factory
            .CreateRenderingParams()
            .ok()
            .map_or(1.8, |params| params.GetGamma())
    };
    let gamma = if gamma.is_finite() && gamma > 1.0 {
        gamma
    } else {
        1.8
    };
    let mut table = [0u8; 256];
    for (level, slot) in table.iter_mut().enumerate() {
        let coverage = level as f32 / 255.0;
        *slot = (coverage.powf(1.0 / gamma) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    table
}

/// Whether the system carries `family` at all.
fn has_family(collection: &IDWriteFontCollection, family: &str) -> bool {
    let name = wide(family);
    let mut index = 0u32;
    let mut exists = windows::core::BOOL(0);
    // SAFETY: a name lookup against a collection the caller owns.
    unsafe {
        collection
            .FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists)
            .is_ok()
            && exists.as_bool()
    }
}

fn with_engine<R>(run: impl FnOnce(&mut Engine) -> Option<R>) -> Option<R> {
    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let engine = slot.get_or_insert_with(Engine::new);
        run(engine.as_mut()?)
    })
}

/// The glyphs of `text` and their advances in pixels at `size`.
fn shape(face: &Face, text: &str, size: f32) -> Option<(Vec<u16>, Vec<f32>)> {
    let points: Vec<u32> = text.chars().map(u32::from).collect();
    if points.is_empty() {
        return None;
    }
    let mut glyphs = vec![0u16; points.len()];
    let mut metrics = vec![DWRITE_GLYPH_METRICS::default(); points.len()];
    // SAFETY: three buffers sized to the codepoint count, handed over with that count.
    unsafe {
        face.face
            .GetGlyphIndices(points.as_ptr(), points.len() as u32, glyphs.as_mut_ptr())
            .ok()?;
        face.face
            .GetDesignGlyphMetrics(
                glyphs.as_ptr(),
                glyphs.len() as u32,
                metrics.as_mut_ptr(),
                false,
            )
            .ok()?;
    }
    let scale = face.scale(size);
    let advances = metrics
        .iter()
        .map(|metric| metric.advanceWidth as f32 * scale)
        .collect();
    Some((glyphs, advances))
}

/// Distance from a line's top to its baseline, in device pixels at `size`.
pub(super) fn ascent_px(size: f32, weight: Weight) -> f32 {
    let size = size.max(1.0);
    with_engine(|engine| {
        let face = engine.face(weight)?;
        Some((face.ascent * face.scale(size)).round())
    })
    .unwrap_or_else(|| (size * 0.8).round())
}

/// Where the middle of a line's visible ink sits below the line's top, in device
/// pixels. Ink runs from the cap line down to the descender, so a glyph centred on the
/// line box rides high beside it; the mac popover lines its header marks up with the
/// ink instead, which is what reads as level.
pub(super) fn ink_middle_px(size: f32, weight: Weight) -> f32 {
    let size = size.max(1.0);
    with_engine(|engine| {
        let face = engine.face(weight)?;
        let scale = face.scale(size);
        let baseline = (face.ascent * scale).round();
        Some(baseline - (face.cap_height - face.descent) * scale / 2.0)
    })
    .unwrap_or(size * 0.55)
}

/// Advance width of `text` in device pixels when drawn at `size`.
pub(super) fn measure_px(text: &str, size: f32, weight: Weight) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let size = size.max(1.0);
    with_engine(|engine| {
        let face = engine.face(weight)?;
        let (_, advances) = shape(face, text, size)?;
        Some(advances.iter().sum::<f32>().round())
    })
    .unwrap_or(0.0)
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
    let size = size.max(1.0);
    let raster = with_engine(|engine| {
        // A cheap refcount bump, so the face lookup can borrow the engine mutably.
        let factory = engine.factory.clone();
        let gamma = engine.gamma;
        let face = engine.face(weight)?;
        let (glyphs, advances) = shape(face, text, size)?;
        let mut raster = rasterize(&factory, face, &glyphs, &advances, size, x, baseline)?;
        for level in &mut raster.coverage {
            *level = gamma[usize::from(*level)];
        }
        Some(raster)
    });
    let Some(raster) = raster else {
        return;
    };
    blend(
        pixmap,
        &raster.coverage,
        raster.width,
        raster.height,
        raster.left,
        raster.top,
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

/// A rasterised run: per-pixel coverage and where it lands on the pixmap.
struct Raster {
    coverage: Vec<u8>,
    width: usize,
    height: usize,
    left: i32,
    top: i32,
}

fn rasterize(
    factory: &IDWriteFactory,
    face: &Face,
    glyphs: &[u16],
    advances: &[f32],
    size: f32,
    x: f32,
    baseline: f32,
) -> Option<Raster> {
    // SAFETY: the run borrows buffers that outlive the analysis, and the font face
    // clone it holds is released before returning.
    unsafe {
        let mut run = DWRITE_GLYPH_RUN {
            fontFace: std::mem::ManuallyDrop::new(Some(face.face.clone())),
            fontEmSize: size,
            glyphCount: glyphs.len() as u32,
            glyphIndices: glyphs.as_ptr(),
            glyphAdvances: advances.as_ptr(),
            glyphOffsets: std::ptr::null(),
            isSideways: false.into(),
            bidiLevel: 0,
        };
        // The rendering Windows itself uses for UI text: ClearType-quality hinting
        // and positioning, with the grid fit on so stems land on whole pixels and stay
        // crisp at 10-13 px, but grayscale output — a layered overlay composites
        // per-pixel alpha and cannot show subpixel colour. Older systems without
        // `IDWriteFactory2` fall back to the plain call.
        let grayscale: Option<IDWriteGlyphRunAnalysis> =
            factory.cast::<IDWriteFactory2>().ok().and_then(|factory| {
                factory
                    .CreateGlyphRunAnalysis(
                        &run,
                        None,
                        DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC,
                        DWRITE_MEASURING_MODE_NATURAL,
                        DWRITE_GRID_FIT_MODE_ENABLED,
                        DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                        x,
                        baseline,
                    )
                    .ok()
            });
        let (analysis, texture) = match grayscale {
            Some(analysis) => (analysis, DWRITE_TEXTURE_ALIASED_1x1),
            None => {
                let analysis = factory.CreateGlyphRunAnalysis(
                    &run,
                    1.0,
                    None,
                    DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC,
                    DWRITE_MEASURING_MODE_NATURAL,
                    x,
                    baseline,
                );
                std::mem::ManuallyDrop::drop(&mut run.fontFace);
                (analysis.ok()?, DWRITE_TEXTURE_CLEARTYPE_3x1)
            }
        };
        if texture == DWRITE_TEXTURE_ALIASED_1x1 {
            std::mem::ManuallyDrop::drop(&mut run.fontFace);
        }
        let bounds: RECT = analysis.GetAlphaTextureBounds(texture).ok()?;
        let width = (bounds.right - bounds.left).max(0) as usize;
        let height = (bounds.bottom - bounds.top).max(0) as usize;
        if width == 0 || height == 0 {
            return None;
        }
        // The grayscale texture is one coverage byte per pixel; the ClearType fallback
        // is three samples that have to be averaged back down to one.
        let samples_per_pixel = if texture == DWRITE_TEXTURE_ALIASED_1x1 {
            1
        } else {
            3
        };
        let mut samples = vec![0u8; width * height * samples_per_pixel];
        analysis
            .CreateAlphaTexture(texture, &bounds, &mut samples)
            .ok()?;
        let coverage = if samples_per_pixel == 1 {
            samples
        } else {
            samples
                .as_chunks::<3>()
                .0
                .iter()
                .map(|triple| {
                    ((u32::from(triple[0]) + u32::from(triple[1]) + u32::from(triple[2])) / 3) as u8
                })
                .collect()
        };
        Some(Raster {
            coverage,
            width,
            height,
            left: bounds.left,
            top: bounds.top,
        })
    }
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

/// Coverage-alpha blending of a rasterised run into a premultiplied pixmap.
fn blend(
    pixmap: &mut Pixmap,
    bitmap: &[u8],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    color: [u8; 4],
) {
    let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
    for row in 0..height {
        let py = y + row as i32;
        if py < 0 || py >= ph {
            continue;
        }
        for col in 0..width {
            let px = x + col as i32;
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
mod tests;
