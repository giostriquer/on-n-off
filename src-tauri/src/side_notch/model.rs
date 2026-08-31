use serde::{Deserialize, Serialize};

#[cfg(any(target_os = "macos", test))]
pub const RAIL_WIDTH: f64 = 76.0;
#[cfg(any(target_os = "macos", test))]
pub const EXPANDED_WIDTH: f64 = 388.0;
#[cfg(any(target_os = "macos", test))]
pub const HEIGHT: f64 = 340.0;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Edge {
    Left,
    #[default]
    Right,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct NotchSettings {
    pub enabled: bool,
    pub display_id: Option<String>,
    pub edge: Edge,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub work_y: f64,
    pub work_height: f64,
    pub scale: f64,
    pub mirrored: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotchSnapshot {
    /// Orders settings reads, writes, and window events within this app run.
    pub revision: u64,
    pub supported: bool,
    pub settings: NotchSettings,
    pub displays: Vec<Display>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg(any(target_os = "macos", test))]
pub struct Layout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[cfg(any(target_os = "macos", test))]
pub fn layout(settings: &NotchSettings, displays: &[Display], expanded: bool) -> Option<Layout> {
    if !settings.enabled {
        return None;
    }
    let id = settings.display_id.as_deref()?;
    let mut matches = displays.iter().filter(|display| display.id == id);
    let display = matches.next()?;
    if matches.next().is_some() || display.mirrored {
        return None;
    }
    let width = if expanded { EXPANDED_WIDTH } else { RAIL_WIDTH }.min(display.width);
    let height = HEIGHT.min(display.work_height);
    if width < RAIL_WIDTH || height < 180.0 {
        return None;
    }
    Some(Layout {
        x: match settings.edge {
            Edge::Left => display.x,
            Edge::Right => display.x + display.width - width,
        },
        y: display.work_y + (display.work_height - height) / 2.0,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: &str, x: f64, scale: f64) -> Display {
        Display {
            id: id.into(),
            name: "Same monitor name".into(),
            x,
            y: 0.0,
            width: 1728.0,
            height: 1117.0,
            work_y: 33.0,
            work_height: 1084.0,
            scale,
            mirrored: false,
        }
    }

    #[test]
    fn follows_the_selected_uuid_across_reordering_and_mixed_scales() {
        let settings = NotchSettings {
            enabled: true,
            display_id: Some("retina".into()),
            edge: Edge::Right,
        };
        let displays = vec![
            display("external", 0.0, 1.0),
            display("retina", -1728.0, 2.0),
        ];
        assert_eq!(
            layout(&settings, &displays, false),
            Some(Layout {
                x: -76.0,
                y: 405.0,
                width: 76.0,
                height: 340.0
            })
        );
        let reversed: Vec<_> = displays.into_iter().rev().collect();
        assert_eq!(
            layout(&settings, &reversed, true),
            Some(Layout {
                x: -388.0,
                y: 405.0,
                width: 388.0,
                height: 340.0
            })
        );
    }

    #[test]
    fn never_falls_back_to_another_display_when_the_selection_is_missing_or_mirrored() {
        let mut settings = NotchSettings {
            enabled: true,
            display_id: Some("missing".into()),
            edge: Edge::Left,
        };
        let mut displays = vec![display("other", 0.0, 1.0)];
        assert_eq!(layout(&settings, &displays, false), None);
        settings.display_id = Some("other".into());
        displays[0].mirrored = true;
        assert_eq!(layout(&settings, &displays, false), None);
    }

    #[test]
    fn keeps_the_left_edge_fixed_when_expanding() {
        let settings = NotchSettings {
            enabled: true,
            display_id: Some("left".into()),
            edge: Edge::Left,
        };
        let displays = vec![display("left", -1728.0, 2.0)];
        assert_eq!(layout(&settings, &displays, true).unwrap().x, -1728.0);
    }

    #[test]
    fn remains_hidden_until_explicitly_enabled() {
        assert_eq!(
            layout(
                &NotchSettings::default(),
                &[display("main", 0.0, 1.0)],
                false
            ),
            None
        );
    }
}
