use super::text::{TextMetrics, TextWeight, Typesetter};
use super::{
    provider_color, tone_color, Color, MarkKind, PopItem, Scale, Zone, MUTED_INK, R, WARN_AMBER,
    WORKSPACE_CREDITS,
};
use crate::dto::AgentId;
use crate::limits::window_at::WindowAt;
use crate::side_notch::model::frame::{CellData, Notice, PrCell, PrList, PrRow, ProviderCell};
use crate::side_notch::model::{meter_color, workspace_share_wording};
use crate::side_notch::sessions::{LiveSession, SessionStatus};
use chrono::{DateTime, Utc};

const WHITE: Color = [255, 255, 255, 255];

struct Stack<'a> {
    at: &'a mut f64,
    spacing: f64,
    after: f64,
}

impl<'a> Stack<'a> {
    fn new(at: &'a mut f64, spacing: f64) -> Self {
        Self {
            at,
            spacing,
            after: spacing,
        }
    }

    fn nest(&mut self, spacing: f64) -> Stack<'_> {
        Stack {
            at: &mut *self.at,
            spacing,
            after: self.spacing,
        }
    }

    fn at(&self) -> f64 {
        *self.at
    }

    fn item(&mut self, extent: f64) -> f64 {
        self.item_then(extent, self.spacing)
    }

    fn last(&mut self, extent: f64) -> f64 {
        self.item_then(extent, self.after)
    }

    fn item_then(&mut self, extent: f64, gap: f64) -> f64 {
        let at = *self.at;
        *self.at += extent + gap;
        at
    }

    fn line(&mut self, extent: f64) -> f64 {
        let at = *self.at;
        *self.at += extent;
        at
    }

    fn space(&mut self) {
        *self.at += self.spacing;
    }

    fn gap(&mut self, gap: f64) {
        *self.at += gap;
    }

    fn close(self) {
        *self.at += self.after - self.spacing;
    }
}

#[derive(Default)]
pub(super) struct Contents {
    pub(super) entries: Vec<(PopItem, R)>,
    pub(super) zones: Vec<(Zone, R)>,
}

impl Contents {
    pub(super) fn bottom(&self) -> f64 {
        self.entries
            .iter()
            .map(|(_, rect)| rect.y + rect.h)
            .fold(0.0_f64, f64::max)
    }

    fn text(&mut self, rect: R, text: String, size: f64, weight: TextWeight, color: Color) {
        self.push_text(rect, text, size, weight, color, false);
    }

    fn text_right(&mut self, rect: R, text: String, size: f64, weight: TextWeight, color: Color) {
        self.push_text(rect, text, size, weight, color, true);
    }

    fn push_text(
        &mut self,
        rect: R,
        text: String,
        size: f64,
        weight: TextWeight,
        color: Color,
        right: bool,
    ) {
        self.entries.push((
            PopItem::Text {
                text,
                size,
                weight,
                color,
                right,
            },
            rect,
        ));
    }

    fn item(&mut self, item: PopItem, rect: R) {
        self.entries.push((item, rect));
    }

    fn zone(&mut self, zone: Zone, rect: R) {
        self.zones.push((zone, rect));
    }
}

struct Block {
    label: String,
    at: WindowAt,
    detail: Option<String>,
}

struct Composer<'a> {
    scale: Scale,
    typeset: Typesetter<'a>,
    now: DateTime<Utc>,
    x: f64,
    inner_w: f64,
    out: Contents,
}

pub(super) fn contents(
    cell: &CellData,
    action_error: Option<&str>,
    inner_w: f64,
    scale: Scale,
    text_metrics: &dyn TextMetrics,
    now: DateTime<Utc>,
) -> Contents {
    let mut composer = Composer {
        scale,
        typeset: Typesetter::new(text_metrics, scale.display),
        now,
        x: scale.value(12.0),
        inner_w,
        out: Contents::default(),
    };
    let mut top = scale.value(12.0);
    let mut card = Stack::new(&mut top, scale.value(10.0));
    let footer = match cell {
        CellData::Provider(provider) => {
            composer.provider(&mut card, provider);
            "Open Limits"
        }
        CellData::PullRequests(pulls) => {
            composer.pull_requests(&mut card, pulls);
            "Open Pull requests"
        }
    };
    composer.footer(&mut card, footer, action_error);
    composer.out
}

impl Composer<'_> {
    fn v(&self, points: f64) -> f64 {
        self.scale.value(points)
    }

    fn line_h(&self, size: f64) -> f64 {
        self.v(size) * 1.3
    }

    fn header(&mut self, at: f64, kind: MarkKind, title: String) {
        let (x, v13) = (self.x, self.v(13.0));
        let lift = self.typeset.cap_middle(13.0, TextWeight::Semibold) - v13 / 2.0;
        self.out.item(
            PopItem::Mark { kind, size: v13 },
            R::new(x, at + lift, v13, v13),
        );
        self.out.text(
            R::new(
                x + v13 + self.v(7.0),
                at,
                self.inner_w - v13 - self.v(7.0),
                self.line_h(13.0),
            ),
            title,
            v13,
            TextWeight::Semibold,
            WHITE,
        );
    }

    fn notice(&mut self, card: &mut Stack, notice: &Notice) {
        let (size, line_h) = (self.v(11.0), self.line_h(11.0));
        for line in self
            .typeset
            .wrap(&notice.text, self.inner_w, size, TextWeight::Regular, 4)
        {
            let at = card.line(line_h);
            self.out.text(
                R::new(self.x, at, self.inner_w, line_h),
                line,
                size,
                TextWeight::Regular,
                tone_color(notice.tone),
            );
        }
    }

    fn provider(&mut self, card: &mut Stack, provider: &ProviderCell) {
        let header = card.item(self.line_h(13.0));
        self.header(
            header,
            MarkKind::Provider(provider.provider),
            format!("{} Usage", provider.provider.display_name()),
        );
        if let Some(notice) = &provider.notice {
            self.notice(card, notice);
        }
        if let Some(paused) = &provider.paused {
            let at = card.item_then(self.line_h(10.0), self.v(4.0));
            self.out.text(
                R::new(self.x, at, self.inner_w, self.line_h(10.0)),
                paused.text.clone(),
                self.v(10.0),
                TextWeight::Regular,
                tone_color(paused.tone),
            );
        }
        let reading = provider.reading.as_ref();
        let mut blocks: Vec<Block> = reading
            .map(|reading| reading.windows.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|window| Block {
                label: window.label.clone(),
                at: WindowAt::of(window, self.now),
                detail: None,
            })
            .collect();
        if let Some(share) = reading.and_then(|reading| reading.workspace_credits.as_ref()) {
            let at = WindowAt::of_share(share, self.now);
            let wording = workspace_share_wording(share);
            blocks.push(Block {
                label: WORKSPACE_CREDITS.into(),
                at,
                detail: Some(if at.renewed() {
                    wording.renewed
                } else {
                    wording.left
                }),
            });
        }
        for block in blocks {
            self.quota_block(card, block, provider.provider);
        }
        if !provider.sessions.is_empty() {
            let divider = card.item(0.0);
            self.out
                .item(PopItem::Divider, R::new(self.x, divider, self.inner_w, 1.0));
            let mut list = card.nest(self.v(8.0));
            for session in &provider.sessions {
                self.session_row(&mut list, session, provider.provider);
            }
            list.close();
        }
    }

    fn quota_block(&mut self, card: &mut Stack, block: Block, provider: AgentId) {
        let (x, inner_w) = (self.x, self.inner_w);
        let note = self.typeset.ellipsize(
            &block.at.notch_note(&chrono::Local),
            inner_w * 0.6,
            self.v(10.0),
            TextWeight::Regular,
        );
        let note_w = self.typeset.width(&note, self.v(10.0), TextWeight::Regular);
        let label_space = (inner_w - note_w - self.v(8.0)).max(inner_w * 0.3);
        let label = self.typeset.ellipsize(
            &block.label,
            label_space,
            self.v(11.0),
            TextWeight::Semibold,
        );
        let mut quota = card.nest(self.v(5.0));
        let heading = quota.item(self.line_h(11.0));
        self.out.text(
            R::new(x, heading, inner_w, self.line_h(11.0)),
            label,
            self.v(11.0),
            TextWeight::Semibold,
            WHITE,
        );
        self.out.text_right(
            R::new(x, heading, inner_w, self.line_h(10.0)),
            note,
            self.v(10.0),
            TextWeight::Regular,
            MUTED_INK,
        );
        let bar = quota.item(self.v(4.0));
        self.out.item(
            PopItem::Bar {
                percent: block.at.used_now(),
                color: meter_color(block.at.meter(), provider_color(provider)),
            },
            R::new(x, bar, inner_w, self.v(4.0)),
        );
        let used = block.at.used_text();
        let figure = match (block.at.used_now(), &block.detail) {
            (None, _) => used,
            (Some(_), None) => format!("{used} Used"),
            (Some(_), Some(detail)) => format!("{used} Used · {detail}"),
        };
        let figure_at = quota.last(self.line_h(10.5));
        self.out.text(
            R::new(x, figure_at, inner_w, self.line_h(10.5)),
            figure,
            self.v(10.5),
            TextWeight::Medium,
            WHITE,
        );
    }

    fn session_row(&mut self, list: &mut Stack, session: &LiveSession, provider: AgentId) {
        let (x, inner_w) = (self.x, self.inner_w);
        let working = session.status == SessionStatus::Working;
        let status_text = if working { "working" } else { "idle" };
        let status_w = self
            .typeset
            .width(status_text, self.v(10.5), TextWeight::Medium);
        let name = self.typeset.ellipsize(
            &session.name,
            inner_w - status_w - self.v(8.0),
            self.v(11.0),
            TextWeight::Semibold,
        );
        let mut row = list.nest(self.v(2.0));
        let name_at = row.item(self.line_h(11.0));
        self.out.text(
            R::new(x, name_at, inner_w, self.line_h(11.0)),
            name,
            self.v(11.0),
            TextWeight::Semibold,
            WHITE,
        );
        self.out.text_right(
            R::new(x, name_at, inner_w, self.line_h(10.5)),
            status_text.into(),
            self.v(10.5),
            TextWeight::Medium,
            if working {
                provider_color(provider)
            } else {
                MUTED_INK
            },
        );
        let detail_at = row.last(self.line_h(10.0));
        self.out.text(
            R::new(x, detail_at, inner_w, self.line_h(10.0)),
            format!("{} · {}", session.place, session.project),
            self.v(10.0),
            TextWeight::Regular,
            MUTED_INK,
        );
        self.out.text_right(
            R::new(x, detail_at, inner_w, self.line_h(10.0)),
            age_text(&session.last_active_at, self.now),
            self.v(10.0),
            TextWeight::Regular,
            MUTED_INK,
        );
    }

    fn pull_requests(&mut self, card: &mut Stack, pulls: &PrCell) {
        let header = card.item(self.line_h(13.0));
        self.header(header, MarkKind::PullRequests, "Pull requests".into());
        if let Some(notice) = &pulls.notice {
            self.notice(card, notice);
            card.gap(self.v(4.0));
        }
        for list in pulls.reading.iter().flat_map(|reading| &reading.lists) {
            self.pull_list(card, list);
            card.gap(self.v(8.0));
        }
    }

    fn pull_list(&mut self, card: &mut Stack, list: &PrList) {
        let (x, inner_w) = (self.x, self.inner_w);
        let mut rows = card.nest(self.v(6.0));
        let heading = rows.item(self.line_h(9.5));
        self.out.text(
            R::new(x, heading, inner_w, self.line_h(9.5)),
            list.title.to_uppercase(),
            self.v(9.5),
            TextWeight::Semibold,
            MUTED_INK,
        );
        self.out.text_right(
            R::new(x, heading, inner_w, self.line_h(9.5)),
            if list.total > list.items.len() as u64 {
                format!("{} of {}", list.items.len(), list.total)
            } else {
                list.items.len().to_string()
            },
            self.v(9.5),
            TextWeight::Regular,
            MUTED_INK,
        );
        if list.items.is_empty() {
            let at = rows.item(self.line_h(10.5));
            self.out.text(
                R::new(x, at, inner_w, self.line_h(10.5)),
                "Nothing open.".into(),
                self.v(10.5),
                TextWeight::Regular,
                MUTED_INK,
            );
        }
        for row in &list.items {
            self.pull_row(&mut rows, row);
        }
    }

    fn pull_row(&mut self, rows: &mut Stack, row: &PrRow) {
        let (x, inner_w) = (self.x, self.inner_w);
        let ink = row.tag.map(|tag| tone_color(tag.tone));
        let title_size = self.v(11.0);
        let copy_w = self.v(20.0);
        let title_w = inner_w - copy_w - self.v(8.0);
        let lines = self
            .typeset
            .wrap(&row.title, title_w, title_size, TextWeight::Semibold, 2);
        let copy_h = self.v(16.0);
        let mut pull = rows.nest(self.v(2.0));
        let row_top = pull.at();
        for (line_index, line) in lines.iter().enumerate() {
            let at = pull.line(self.line_h(11.0));
            self.out.text(
                R::new(x, at, title_w, self.line_h(11.0)),
                line.clone(),
                title_size,
                TextWeight::Semibold,
                WHITE,
            );
            if line_index == 0 {
                self.out.item(
                    PopItem::CopyIcon {
                        color: ink.unwrap_or(MUTED_INK),
                    },
                    R::new(x + inner_w - copy_w, at, copy_w, copy_h),
                );
                self.out.zone(
                    Zone::CopyRow {
                        url: row.url.clone(),
                        title: row.title.clone(),
                    },
                    R::new(x + inner_w - copy_w, at, copy_w, copy_h),
                );
            }
        }
        pull.space();
        let repo_at = pull.last(self.line_h(10.0));
        let repo_size = self.v(10.0);
        let badge_limit = x + inner_w - self.v(7.0) - self.v(10.0);
        let repo = self.typeset.ellipsize(
            &format!("{} #{}", row.repo, row.number),
            badge_limit - x,
            repo_size,
            TextWeight::Regular,
        );
        let mut badge_x =
            x + self.typeset.width(&repo, repo_size, TextWeight::Regular) + self.v(8.0);
        let mut along = Stack::new(&mut badge_x, self.v(6.0));
        self.out.text(
            R::new(x, repo_at, inner_w, self.line_h(10.0)),
            repo,
            repo_size,
            TextWeight::Regular,
            MUTED_INK,
        );
        for badge in &row.badges {
            let badge_size = self.v(9.5);
            let badge_w = self
                .typeset
                .width(badge.label, badge_size, TextWeight::Medium);
            if along.at() + badge_w > badge_limit {
                break;
            }
            let at = along.item(badge_w);
            self.out.text(
                R::new(at, repo_at, inner_w, self.line_h(9.5)),
                badge.label.into(),
                badge_size,
                TextWeight::Medium,
                tone_color(badge.tone),
            );
        }
        self.out.item(
            PopItem::Dot { ci: row.ci },
            R::new(
                x + inner_w - self.v(7.0),
                repo_at + self.typeset.cap_middle(10.0, TextWeight::Regular) - self.v(7.0) / 2.0,
                self.v(7.0),
                self.v(7.0),
            ),
        );
        let row_rect = R::new(x, row_top, inner_w, repo_at + self.line_h(10.0) - row_top);
        if let Some(ink) = ink {
            self.out.item(
                PopItem::Stripe { color: ink },
                R::new(x - self.v(8.0), row_rect.y, self.v(2.0), row_rect.h),
            );
        }
        self.out.zone(
            Zone::OpenRow {
                url: row.url.clone(),
            },
            row_rect,
        );
    }

    fn footer(&mut self, card: &mut Stack, title: &str, action_error: Option<&str>) {
        let (x, inner_w) = (self.x, self.inner_w);
        if let Some(error) = action_error {
            let at = card.item_then(self.line_h(10.0), 0.0);
            self.out.text(
                R::new(x, at, inner_w, self.line_h(10.0)),
                error.into(),
                self.v(10.0),
                TextWeight::Regular,
                WARN_AMBER,
            );
        }
        let footer = card.at();
        let size = self.v(10.5);
        let line = self.line_h(10.5);
        let arrow = self.v(9.0);
        let label_w = self.typeset.width(title, size, TextWeight::Medium);
        let footer_w = label_w + self.v(3.0) + arrow;
        let footer_x = x + inner_w - footer_w;
        self.out.text(
            R::new(footer_x, footer, label_w, line),
            title.into(),
            size,
            TextWeight::Medium,
            MUTED_INK,
        );
        self.out.item(
            PopItem::Mark {
                kind: MarkKind::OpenArrow,
                size: arrow,
            },
            R::new(
                x + inner_w - arrow,
                footer + self.typeset.cap_middle(10.5, TextWeight::Medium) - arrow / 2.0,
                arrow,
                arrow,
            ),
        );
        self.out
            .zone(Zone::Footer, R::new(footer_x, footer, footer_w, line));
    }
}

fn age_text(last_active_at: &str, now: DateTime<Utc>) -> String {
    let Some(instant) = last_active_at.parse::<DateTime<Utc>>().ok() else {
        return String::new();
    };
    let minutes = (now - instant).num_minutes();
    if minutes < 1 {
        "just now".into()
    } else if minutes < 60 {
        format!("{minutes} min")
    } else if minutes < 1440 {
        format!("{} h", minutes / 60)
    } else {
        format!("{} d", minutes / 1440)
    }
}
