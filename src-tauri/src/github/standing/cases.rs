use serde::Deserialize;

use crate::dto::{
    GithubPrDto, GithubPrListDto, GithubPrsData, GithubPrsDto, GithubStatus, PrReadStanding,
};

const FIXTURE: &str = include_str!("fixtures/standing-cases.json");

#[derive(Deserialize)]
pub(crate) struct Fixture {
    pub(crate) cases: Vec<Case>,
}

#[derive(Deserialize)]
pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) status: GithubStatus,
    hint: Option<String>,
    stale: bool,
    pub(crate) rows: usize,
    pub(crate) standing: serde_json::Value,
    pub(crate) notch: serde_json::Value,
}

pub(crate) fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).expect("the pull request read standing cases")
}

impl Case {
    pub(crate) fn read(&self) -> GithubPrsDto {
        self.read_over(vec![row()])
    }

    pub(crate) fn read_over(&self, rows: Vec<GithubPrDto>) -> GithubPrsDto {
        let rows = if self.status == GithubStatus::Ok || self.stale {
            rows.into_iter().take(self.rows).collect()
        } else {
            Vec::new()
        };
        GithubPrsDto {
            status: self.status,
            hint: self.hint.clone(),
            stale: self.stale,
            data: GithubPrsData {
                mine: GithubPrListDto {
                    total: rows.len() as u64,
                    items: rows,
                },
                ..GithubPrsData::default()
            },
            warnings: Vec::new(),
        }
    }

    pub(crate) fn expected(&self) -> PrReadStanding {
        serde_json::from_value(self.standing.clone()).expect("a pull request read standing")
    }
}

fn row() -> GithubPrDto {
    serde_json::from_value(serde_json::json!({
        "id": "node-1", "number": 1, "title": "Fix the build",
        "url": "https://github.com/acme/webapp/pull/1", "repo": "acme/webapp",
        "author": "octocat", "isDraft": false, "ci": "success", "headRef": "fix",
        "baseRef": "main", "updatedAt": "2026-09-01T10:00:00Z"
    }))
    .expect("a pull request row")
}
