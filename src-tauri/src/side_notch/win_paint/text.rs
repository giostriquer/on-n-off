#![allow(unsafe_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use tiny_skia::Pixmap;
use windows::core::PCWSTR;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_TEXTURE_CLEARTYPE_3x1, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteFontFace, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_METRICS, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_GLYPH_METRICS,
    DWRITE_GLYPH_RUN, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Weight {
    Regular,
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

const NOTCH_FACE: &str = "Segoe UI";
const FALLBACK_FACE: &str = "Tahoma";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Face {
    face: IDWriteFontFace,
    units_per_em: f32,
    ascent: f32,
    cap_height: f32,
}

impl Face {
    fn scale(&self, size: f32) -> f32 {
        size / self.units_per_em
    }
}

struct Engine {
    factory: IDWriteFactory,
    collection: IDWriteFontCollection,
    family: &'static str,
    faces: HashMap<Weight, Face>,
    gamma: [u8; 256],
}

thread_local! {
    static ENGINE: RefCell<Option<Option<Engine>>> = const { RefCell::new(None) };
}

impl Engine {
    fn new() -> Option<Self> {
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
            let gamma = gamma_table();
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
        let mut metrics = DWRITE_FONT_METRICS::default();
        unsafe { face.GetMetrics(&mut metrics) };
        Some(Face {
            face,
            units_per_em: f32::from(metrics.designUnitsPerEm.max(1)),
            ascent: f32::from(metrics.ascent),
            cap_height: f32::from(metrics.capHeight),
        })
    }

    fn installed_face(&self, weight: Weight) -> Option<IDWriteFontFace> {
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
            family
                .GetFirstMatchingFont(
                    weight.dwrite(),
                    DWRITE_FONT_STRETCH_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                )
                .ok()?
                .CreateFontFace()
                .ok()
        }
    }
}

fn gamma_table() -> [u8; 256] {
    const GAMMA: f32 = 2.2;
    let mut table = [0u8; 256];
    for (level, slot) in table.iter_mut().enumerate() {
        let coverage = level as f32 / 255.0;
        *slot = (coverage.powf(1.0 / GAMMA) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    table
}

fn has_family(collection: &IDWriteFontCollection, family: &str) -> bool {
    let name = wide(family);
    let mut index = 0u32;
    let mut exists = windows::core::BOOL(0);
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

fn shape(face: &Face, text: &str, size: f32) -> Option<(Vec<u16>, Vec<f32>)> {
    let points: Vec<u32> = text.chars().map(u32::from).collect();
    if points.is_empty() {
        return None;
    }
    let mut glyphs = vec![0u16; points.len()];
    let mut metrics = vec![DWRITE_GLYPH_METRICS::default(); points.len()];
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

pub(super) fn ascent_px(size: f32, weight: Weight) -> f32 {
    let size = size.max(1.0);
    with_engine(|engine| {
        let face = engine.face(weight)?;
        Some((face.ascent * face.scale(size)).round())
    })
    .unwrap_or_else(|| (size * 0.8).round())
}

pub(super) fn cap_middle_px(size: f32, weight: Weight) -> f32 {
    let size = size.max(1.0);
    with_engine(|engine| {
        let face = engine.face(weight)?;
        let scale = face.scale(size);
        let baseline = (face.ascent * scale).round();
        Some(baseline - face.cap_height * scale / 2.0)
    })
    .unwrap_or(size * 0.45)
}

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
        let analysis = factory.CreateGlyphRunAnalysis(
            &run,
            1.0,
            None,
            DWRITE_RENDERING_MODE_CLEARTYPE_NATURAL_SYMMETRIC,
            DWRITE_MEASURING_MODE_NATURAL,
            x,
            baseline,
        );
        std::mem::ManuallyDrop::drop(&mut run.fontFace);
        let analysis = analysis.ok()?;
        let bounds: RECT = analysis
            .GetAlphaTextureBounds(DWRITE_TEXTURE_CLEARTYPE_3x1)
            .ok()?;
        let width = (bounds.right - bounds.left).max(0) as usize;
        let height = (bounds.bottom - bounds.top).max(0) as usize;
        if width == 0 || height == 0 {
            return None;
        }
        let mut samples = vec![0u8; width * height * 3];
        analysis
            .CreateAlphaTexture(DWRITE_TEXTURE_CLEARTYPE_3x1, &bounds, &mut samples)
            .ok()?;
        Some(Raster {
            coverage: samples
                .as_chunks::<3>()
                .0
                .iter()
                .map(|triple| luminance(*triple))
                .collect(),
            width,
            height,
            left: bounds.left,
            top: bounds.top,
        })
    }
}

fn luminance(triple: [u8; 3]) -> u8 {
    let [r, g, b] = triple.map(u32::from);
    ((r * 54 + g * 183 + b * 19) >> 8) as u8
}

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
