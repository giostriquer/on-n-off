#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextWeight {
    Regular,
    Medium,
    Semibold,
}

pub trait TextMetrics {
    fn measure_px(&self, text: &str, size: f32, weight: TextWeight) -> f32;
    fn cap_middle_px(&self, size: f32, weight: TextWeight) -> f32;
}

#[derive(Clone, Copy)]
pub(super) struct Typesetter<'a> {
    metrics: &'a dyn TextMetrics,
    scale: f64,
}

impl<'a> Typesetter<'a> {
    pub(super) fn new(metrics: &'a dyn TextMetrics, scale: f64) -> Self {
        Self { metrics, scale }
    }

    pub(super) fn width(&self, text: &str, size: f64, weight: TextWeight) -> f64 {
        f64::from(
            self.metrics
                .measure_px(text, device_size(size, self.scale), weight),
        ) / self.scale
    }

    pub(super) fn cap_middle(&self, size: f64, weight: TextWeight) -> f64 {
        f64::from(
            self.metrics
                .cap_middle_px(device_size(size, self.scale), weight),
        ) / self.scale
    }

    pub(super) fn wrap(
        &self,
        text: &str,
        max_w: f64,
        size: f64,
        weight: TextWeight,
        max_lines: usize,
    ) -> Vec<String> {
        if size <= 0.0 {
            return vec![text.to_string()];
        }
        wrap_px(
            self.metrics,
            text,
            (max_w * self.scale) as f32,
            device_size(size, self.scale),
            weight,
            max_lines,
        )
    }

    pub(super) fn ellipsize(
        &self,
        text: &str,
        max_w: f64,
        size: f64,
        weight: TextWeight,
    ) -> String {
        if self.width(text, size, weight) <= max_w {
            return text.to_string();
        }
        let mut cut = text.to_string();
        while cut.chars().count() > 1 && self.width(&format!("{cut}…"), size, weight) > max_w {
            cut.pop();
        }
        format!("{cut}…")
    }
}

fn device_size(size: f64, scale: f64) -> f32 {
    (size * scale).max(1.0) as f32
}

pub(super) fn wrap_px(
    metrics: &dyn TextMetrics,
    text: &str,
    max_px: f32,
    size: f32,
    weight: TextWeight,
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
        if metrics.measure_px(&candidate, size, weight) <= max_px || current.is_empty() {
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
            while !cut.is_empty() && metrics.measure_px(&format!("{cut}…"), size, weight) > max_px
            {
                cut.pop();
            }
            *last = format!("{cut}…");
        }
    }
    lines
}

#[cfg(test)]
pub struct FixedAdvance;

#[cfg(test)]
impl FixedAdvance {
    fn em(weight: TextWeight) -> f32 {
        match weight {
            TextWeight::Regular => 0.5,
            TextWeight::Medium => 0.55,
            TextWeight::Semibold => 0.6,
        }
    }
}

#[cfg(test)]
impl TextMetrics for FixedAdvance {
    fn measure_px(&self, text: &str, size: f32, weight: TextWeight) -> f32 {
        let advance = size.max(1.0) * Self::em(weight);
        (text.chars().count() as f32 * advance).round()
    }

    fn cap_middle_px(&self, size: f32, _weight: TextWeight) -> f32 {
        let size = size.max(1.0);
        (size * 1.08).round() - size * 0.35
    }
}
