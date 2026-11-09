//! What the board knows about a PR, independent of how it was fetched.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::kind::Kind;
use crate::size::ReviewSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ci {
    Passing,
    Failing,
    Running,
    None,
}

impl Ci {
    pub fn from_rollup(state: Option<&str>) -> Self {
        match state {
            Some("SUCCESS") => Ci::Passing,
            Some("FAILURE") | Some("ERROR") => Ci::Failing,
            Some("PENDING") | Some("EXPECTED") => Ci::Running,
            _ => Ci::None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Ci::Passing => "passing",
            Ci::Failing => "failing",
            Ci::Running => "running",
            Ci::None => "no checks",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Approved,
    ChangesRequested,
    Required,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub decision: Decision,
    pub approvals: u32,
    /// Reviewers (users or teams) still asked to review.
    pub pending: u32,
}

impl Review {
    pub fn word(&self) -> String {
        match self.decision {
            Decision::Approved => format!("approved ×{}", self.approvals.max(1)),
            Decision::ChangesRequested => "changes requested".into(),
            Decision::Required if self.pending > 0 => format!("{} pending", self.pending),
            Decision::Required => "review required".into(),
            Decision::None if self.approvals > 0 => format!("{} approval(s)", self.approvals),
            Decision::None if self.pending > 0 => format!("{} pending", self.pending),
            Decision::None => "no reviews".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Merge {
    Ready,
    Conflict,
    Behind,
    Blocked,
    Unstable,
    Draft,
    Unknown,
}

impl Merge {
    pub fn from_github(mergeable: Option<&str>, state: Option<&str>, draft: bool) -> Self {
        if draft {
            return Merge::Draft;
        }
        if mergeable == Some("CONFLICTING") {
            return Merge::Conflict;
        }
        match state {
            Some("CLEAN") | Some("HAS_HOOKS") => Merge::Ready,
            Some("DIRTY") => Merge::Conflict,
            Some("BEHIND") => Merge::Behind,
            Some("BLOCKED") => Merge::Blocked,
            Some("UNSTABLE") => Merge::Unstable,
            Some("DRAFT") => Merge::Draft,
            _ => Merge::Unknown,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Merge::Ready => "ready to merge",
            Merge::Conflict => "conflicts",
            Merge::Behind => "behind base",
            Merge::Blocked => "blocked",
            Merge::Unstable => "unstable",
            Merge::Draft => "draft",
            Merge::Unknown => "checking…",
        }
    }
}

/// The winning CR-kind request on a PR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub kind: Kind,
    pub why: Option<String>,
    pub by: Option<String>,
    pub at: DateTime<Utc>,
}

/// Conversation state, fetched for the "My PRs" screen only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub unresolved_threads: u32,
    pub total_threads: u32,
    pub comments: u32,
    /// Latest comment, review or thread reply by anyone but the PR author.
    pub last_by_others: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pr {
    pub id: String,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub additions: u64,
    pub deletions: u64,
    /// The repo's `review: S|M|L|XL` reading-load size, when its CI sets one.
    #[serde(default)]
    pub review_size: Option<ReviewSize>,
    pub draft: bool,
    pub ci: Ci,
    pub review: Review,
    pub merge: Merge,
    pub request: Option<Request>,
    pub activity: Option<Activity>,
}

impl Pr {
    pub fn key(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }

    pub fn kind(&self) -> Option<Kind> {
        self.request.as_ref().map(|r| r.kind)
    }

    /// Size for list columns: the review label when there is one, else the
    /// raw diff (`+120 −30`).
    pub fn size_short(&self) -> String {
        match &self.review_size {
            Some(s) => s.label(),
            None => self.raw_size(),
        }
    }

    /// Size for detail panes: the review label with the raw diff alongside.
    pub fn size_long(&self) -> String {
        match &self.review_size {
            Some(s) => format!("{} · {} raw", s.label(), self.raw_size()),
            None => self.raw_size(),
        }
    }

    fn raw_size(&self) -> String {
        format!("+{} −{}", self.additions, self.deletions)
    }

    /// The repo part after the slash; orgs make the owner redundant.
    pub fn short_repo(&self) -> &str {
        self.repo.rsplit('/').next().unwrap_or(&self.repo)
    }
}

/// Compact age like `now`, `5m`, `3h`, `2d`, `6w`.
pub fn age(from: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - from).num_seconds().max(0);
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        86_400..=1_209_599 => format!("{}d", secs / 86_400),
        _ if secs / 604_800 > 99 => "99w".into(),
        _ => format!("{}w", secs / 604_800),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn ages() {
        let now = Utc::now();
        assert_eq!(age(now, now), "now");
        assert_eq!(age(now - Duration::minutes(5), now), "5m");
        assert_eq!(age(now - Duration::hours(3), now), "3h");
        assert_eq!(age(now - Duration::days(2), now), "2d");
        assert_eq!(age(now - Duration::days(20), now), "2w");
        assert_eq!(age(now - Duration::days(5000), now), "99w");
        assert_eq!(age(now + Duration::hours(1), now), "now");
    }

    #[test]
    fn merge_precedence() {
        assert_eq!(
            Merge::from_github(Some("CONFLICTING"), Some("BEHIND"), true),
            Merge::Draft
        );
        assert_eq!(
            Merge::from_github(Some("CONFLICTING"), Some("BEHIND"), false),
            Merge::Conflict
        );
        assert_eq!(
            Merge::from_github(Some("MERGEABLE"), Some("CLEAN"), false),
            Merge::Ready
        );
        assert_eq!(Merge::from_github(None, None, false), Merge::Unknown);
    }

    #[test]
    fn ci_from_rollup() {
        assert_eq!(Ci::from_rollup(Some("ERROR")), Ci::Failing);
        assert_eq!(Ci::from_rollup(Some("EXPECTED")), Ci::Running);
        assert_eq!(Ci::from_rollup(None), Ci::None);
    }
}
