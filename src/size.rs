//! Review size: the reading-load size some repos' CI puts on every PR.
//!
//! Autofleet's autorepo CI (`ci-scripts/review-effort.mjs`, rationale in its
//! `docs/pr-review-effort.md`) sets one `review: S|M|L|XL` label per PR,
//! bucketed at 25/160/700 lines of added code to read, and keeps one comment
//! up to date:
//!
//! ```text
//! <!-- review-effort -->
//! 🟢 **Review: S** (~18 lines of added code to read)
//! 🕸️ **Hubs:** `ordering: CatalogPage/CatalogStore.ts` (18 importers)
//! ```
//!
//! The label is the signal; the comment only adds the line count and hub
//! files. PR Begger reads both and never writes either.

use serde::{Deserialize, Serialize};

/// What the CI script starts its comment with, and searches for to update it.
const COMMENT_MARKER: &str = "<!-- review-effort -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Bucket {
    S,
    M,
    L,
    XL,
}

impl Bucket {
    fn from_name(s: &str) -> Option<Self> {
        match s {
            "S" => Some(Bucket::S),
            "M" => Some(Bucket::M),
            "L" => Some(Bucket::L),
            "XL" => Some(Bucket::XL),
            _ => None,
        }
    }

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
        Self::from_name(label.strip_prefix("review: ")?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSize {
    pub bucket: Bucket,
    /// Lines of added code to read, from the comment.
    pub lines: Option<u32>,
    /// The comment's hub line, e.g. `ordering: CatalogStore.ts (18 importers)`.
    pub hubs: Option<String>,
}

impl ReviewSize {
    /// From a PR's label names and comment bodies. The label wins; the
    /// comment fills in detail when it agrees with it, and stands in alone
    /// when the label is missing.
    pub fn resolve<'a>(
        labels: impl IntoIterator<Item = &'a str>,
        comments: impl IntoIterator<Item = &'a str>,
    ) -> Option<Self> {
        let labelled: Vec<Bucket> = labels.into_iter().filter_map(Bucket::from_label).collect();
        let comment = comments.into_iter().filter_map(parse_comment).last();
        let bucket = match (labelled.as_slice(), &comment) {
            ([], c) => return c.clone(),
            ([one], _) => *one,
            // The script adds the new label before removing the old one, so
            // two can coexist briefly: trust the comment, else the bigger.
            (many, Some(c)) if many.contains(&c.bucket) => c.bucket,
            (many, _) => *many.iter().max()?,
        };
        Some(match comment {
            Some(c) if c.bucket == bucket => c,
            _ => ReviewSize {
                bucket,
                lines: None,
                hubs: None,
            },
        })
    }

    /// For list columns: `review: M`.
    pub fn short(&self) -> String {
        format!("review: {}", self.bucket.name())
    }

    /// For detail panes: `review: M (~48 lines to read)`.
    pub fn long(&self) -> String {
        match self.lines {
            Some(n) => format!("{} (~{n} lines to read)", self.short()),
            None => self.short(),
        }
    }
}

/// The review-effort comment's size line and hub line, if `body` is one.
fn parse_comment(body: &str) -> Option<ReviewSize> {
    let rest = body.trim_start().strip_prefix(COMMENT_MARKER)?;
    let mut bucket = None;
    let mut lines = None;
    let mut hubs = None;
    for line in rest.lines() {
        if bucket.is_none() {
            if let Some(after) = line.split_once("**Review: ").map(|(_, a)| a) {
                let (name, tail) = after.split_once("**")?;
                bucket = Some(Bucket::from_name(name)?);
                lines = tail
                    .trim_start()
                    .strip_prefix("(~")
                    .and_then(|t| t.split_once(' '))
                    .and_then(|(n, _)| n.parse().ok());
                continue;
            }
        }
        if let Some((_, h)) = line.split_once("**Hubs:**") {
            let h = h.replace('`', "");
            let h = h.trim();
            if !h.is_empty() {
                hubs = Some(h.to_string());
            }
        }
    }
    Some(ReviewSize {
        bucket: bucket?,
        lines,
        hubs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMENT: &str = "<!-- review-effort -->\n\
        🟢 **Review: S** (~18 lines of added code to read)\n\
        🕸️ **Hubs:** `ordering: CatalogPage/CatalogStore.ts` (18 importers)\n\
        🤖 _Small change to a hub file; consider `claude-review`._\n\
        <!-- Reading load and hub files from the diff, not a verdict. -->";

    fn size(bucket: Bucket, lines: Option<u32>, hubs: Option<&str>) -> Option<ReviewSize> {
        Some(ReviewSize {
            bucket,
            lines,
            hubs: hubs.map(String::from),
        })
    }

    #[test]
    fn parses_the_ci_comment() {
        assert_eq!(
            parse_comment(COMMENT),
            size(
                Bucket::S,
                Some(18),
                Some("ordering: CatalogPage/CatalogStore.ts (18 importers)")
            )
        );
    }

    #[test]
    fn parses_raw_and_heads_up_variants() {
        let body = "<!-- review-effort -->\n\
            🟠 **Review: L** (~229 lines of added code to read; 1450 raw: 900 data/fixture)\n\
            ⚠️ **Heads up:** touches CI";
        assert_eq!(parse_comment(body), size(Bucket::L, Some(229), None));
    }

    #[test]
    fn ignores_other_comments() {
        assert_eq!(parse_comment("🟢 **Review: S** (~18 lines)"), None);
        assert_eq!(
            parse_comment("> <!-- review-effort -->\n**Review: S**"),
            None
        );
        assert_eq!(
            parse_comment("<!-- review-effort -->\n**Review: XXL**"),
            None
        );
        assert_eq!(parse_comment("<!-- review-effort -->\nnothing"), None);
    }

    #[test]
    fn unreadable_line_count_keeps_the_bucket() {
        let body = "<!-- review-effort -->\n🔴 **Review: XL** (lots)";
        assert_eq!(parse_comment(body), size(Bucket::XL, None, None));
    }

    #[test]
    fn label_alone() {
        assert_eq!(
            ReviewSize::resolve(["bug", "review: M"], []),
            size(Bucket::M, None, None)
        );
        assert_eq!(ReviewSize::resolve(["review: huge", "Review: S"], []), None);
    }

    #[test]
    fn comment_adds_detail_when_it_agrees() {
        assert_eq!(
            ReviewSize::resolve(["review: S"], ["hi", COMMENT]),
            parse_comment(COMMENT)
        );
        // a label someone changed by hand outranks the comment's numbers
        assert_eq!(
            ReviewSize::resolve(["review: XL"], [COMMENT]),
            size(Bucket::XL, None, None)
        );
    }

    #[test]
    fn comment_without_label() {
        assert_eq!(ReviewSize::resolve([], [COMMENT]), parse_comment(COMMENT));
    }

    #[test]
    fn two_labels_mid_update() {
        assert_eq!(
            ReviewSize::resolve(["review: L", "review: S"], [COMMENT])
                .unwrap()
                .bucket,
            Bucket::S
        );
        assert_eq!(
            ReviewSize::resolve(["review: L", "review: S"], [])
                .unwrap()
                .bucket,
            Bucket::L
        );
    }

    #[test]
    fn nothing_to_go_on() {
        assert_eq!(ReviewSize::resolve(["bug"], ["lgtm"]), None);
    }

    #[test]
    fn display() {
        let s = parse_comment(COMMENT).unwrap();
        assert_eq!(s.short(), "review: S");
        assert_eq!(s.long(), "review: S (~18 lines to read)");
        assert_eq!(size(Bucket::XL, None, None).unwrap().long(), "review: XL");
    }
}
