//! Review size: the reading-load size some repos' CI puts on every PR.
//!
//! Autofleet's autorepo CI (`ci-scripts/review-effort.mjs`, rationale in its
//! `docs/pr-review-effort.md`) sets one `review: S|M|L|XL` label per PR,
//! bucketed at 25/160/700 lines of added code to read. PR Begger only reads
//! the label; it never sets or changes it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Bucket {
    S,
    M,
    L,
    XL,
}

impl Bucket {
    pub fn name(self) -> &'static str {
        match self {
            Bucket::S => "S",
            Bucket::M => "M",
            Bucket::L => "L",
            Bucket::XL => "XL",
        }
    }

    /// `review: M` → `M`; any other label → `None`.
    fn from_label(label: &str) -> Option<Self> {
        match label.strip_prefix("review: ")? {
            "S" => Some(Bucket::S),
            "M" => Some(Bucket::M),
            "L" => Some(Bucket::L),
            "XL" => Some(Bucket::XL),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSize {
    pub bucket: Bucket,
}

impl ReviewSize {
    /// From a PR's label names; `None` without a `review:` label. The CI
    /// adds the new label before removing the old one, so two can coexist
    /// briefly: take the bigger.
    pub fn from_labels<'a>(labels: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        labels
            .into_iter()
            .filter_map(Bucket::from_label)
            .max()
            .map(|bucket| ReviewSize { bucket })
    }

    /// `review: M`
    pub fn label(&self) -> String {
        format!("review: {}", self.bucket.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(labels: &[&str]) -> Option<Bucket> {
        ReviewSize::from_labels(labels.iter().copied()).map(|s| s.bucket)
    }

    #[test]
    fn reads_the_review_label() {
        assert_eq!(bucket(&["bug", "review: M"]), Some(Bucket::M));
        assert_eq!(bucket(&["review: XL"]), Some(Bucket::XL));
    }

    #[test]
    fn ignores_other_labels() {
        assert_eq!(bucket(&[]), None);
        assert_eq!(bucket(&["bug", "size: S"]), None);
        assert_eq!(bucket(&["review: huge", "Review: S", "review:S"]), None);
    }

    #[test]
    fn two_labels_mid_update_takes_the_bigger() {
        assert_eq!(bucket(&["review: S", "review: L"]), Some(Bucket::L));
    }

    #[test]
    fn label_text() {
        let s = ReviewSize::from_labels(["review: S"]).unwrap();
        assert_eq!(s.label(), "review: S");
    }
}
