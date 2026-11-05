//! The PR comment that carries the CR-kind signal.
//!
//! A request comment has a hidden machine marker on its first line followed
//! by a human-readable line:
//!
//! ```text
//! <!-- pr-begger: {"v":1,"kind":"design","why":"new queue topology"} -->
//! 🧭 **CR kind: Design review** - new queue topology
//! ```
//!
//! The marker is the source of truth; the human text is for people reading
//! the PR on GitHub. When a PR has several marker comments the newest one
//! wins, where "newest" is the last edit (or creation) time.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::kind::Kind;

pub const MARKER_PREFIX: &str = "<!-- pr-begger:";
const MARKER_SUFFIX: &str = "-->";
/// Current marker schema version.
pub const VERSION: u32 = 1;
/// Plain word every request comment carries in visible text, so GitHub's
/// comment search can find it (the search index is not guaranteed to cover
/// HTML comments).
pub const SEARCH_TOKEN: &str = "pr-begger";
const MAX_WHY_CHARS: usize = 280;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub v: u32,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl Marker {
    pub fn new(kind: Kind, why: Option<&str>) -> Self {
        Marker {
            v: VERSION,
            kind,
            why: why.map(clean_why).filter(|w| !w.is_empty()),
        }
    }

    /// The hidden HTML-comment line. `<`, `>` and `&` are written as JSON
    /// unicode escapes so no reason text can close the HTML comment early.
    pub fn to_marker_line(&self) -> String {
        let json = serde_json::to_string(self).expect("marker serializes");
        let json = json
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('&', "\\u0026");
        format!("{MARKER_PREFIX} {json} {MARKER_SUFFIX}")
    }

    /// The full comment body posted to the PR.
    pub fn to_comment_body(&self) -> String {
        let mut human = format!("{} **CR kind: {}**", self.kind.emoji(), self.kind.label());
        if let Some(why) = &self.why {
            human.push_str(" - ");
            human.push_str(&escape_markdown_line(why));
        }
        format!(
            "{}\n{}\n\n<sub>{} · requested with {} · run `pr-begger request` again to change it</sub>\n",
            self.to_marker_line(),
            human,
            self.kind.blurb(),
            SEARCH_TOKEN,
        )
    }
}

/// Collapse a reason to one tidy line.
fn clean_why(why: &str) -> String {
    let one_line = why.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > MAX_WHY_CHARS {
        let cut: String = one_line.chars().take(MAX_WHY_CHARS - 1).collect();
        format!("{cut}…")
    } else {
        one_line
    }
}

/// Keep a user-supplied reason from turning into HTML or a heading on GitHub.
fn escape_markdown_line(s: &str) -> String {
    s.replace('<', "&lt;").replace('>', "&gt;")
}

/// Parse the marker out of a comment body, if it has one. Unknown kinds and
/// malformed JSON yield `None`: a marker we cannot read is not a signal.
pub fn parse(body: &str) -> Option<Marker> {
    let start = body.find(MARKER_PREFIX)? + MARKER_PREFIX.len();
    let rest = &body[start..];
    let end = rest.find(MARKER_SUFFIX)?;
    let marker: Marker = serde_json::from_str(rest[..end].trim()).ok()?;
    if marker.v == 0 {
        return None;
    }
    Some(marker)
}

/// A PR comment as far as the marker logic cares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub author: Option<String>,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub last_edited_at: Option<DateTime<Utc>>,
}

impl Comment {
    /// When this comment last said what it says now.
    pub fn effective_at(&self) -> DateTime<Utc> {
        self.last_edited_at
            .unwrap_or(self.created_at)
            .max(self.created_at)
    }
}

/// The winning request on a PR: the newest marker comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found<'a> {
    pub comment: &'a Comment,
    pub marker: Marker,
}

/// Newest-wins: of all comments carrying a readable marker, the one with the
/// latest effective time. Ties go to the later comment in thread order.
pub fn newest(comments: &[Comment]) -> Option<Found<'_>> {
    let mut best: Option<Found<'_>> = None;
    for c in comments {
        let Some(marker) = parse(&c.body) else {
            continue;
        };
        let replace = match &best {
            None => true,
            Some(b) => c.effective_at() >= b.comment.effective_at(),
        };
        if replace {
            best = Some(Found { comment: c, marker });
        }
    }
    best
}

/// What `request` should do: edit the viewer's own newest marker comment, or
/// post a fresh one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Create,
    Update { comment_id: String },
}

pub fn plan_request(comments: &[Comment], viewer: &str) -> Plan {
    let mine: Vec<Comment> = comments
        .iter()
        .filter(|c| {
            c.author
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(viewer))
        })
        .cloned()
        .collect();
    match newest(&mine) {
        Some(found) => Plan::Update {
            comment_id: found.comment.id.clone(),
        },
        None => Plan::Create,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 5, h, 0, 0).unwrap()
    }

    fn comment(id: &str, author: &str, body: &str, created: u32, edited: Option<u32>) -> Comment {
        Comment {
            id: id.into(),
            author: Some(author.into()),
            body: body.into(),
            created_at: at(created),
            last_edited_at: edited.map(at),
        }
    }

    fn body(kind: Kind) -> String {
        Marker::new(kind, None).to_comment_body()
    }

    #[test]
    fn serializes_expected_marker_line() {
        let m = Marker::new(Kind::Design, None);
        assert_eq!(
            m.to_marker_line(),
            r#"<!-- pr-begger: {"v":1,"kind":"design"} -->"#
        );
    }

    #[test]
    fn round_trips_every_kind_with_and_without_why() {
        for k in Kind::ALL {
            for why in [None, Some("touches the billing schema")] {
                let m = Marker::new(k, why);
                assert_eq!(parse(&m.to_comment_body()), Some(m.clone()));
                assert_eq!(parse(&m.to_marker_line()), Some(m));
            }
        }
    }

    #[test]
    fn human_line_is_readable() {
        let b = Marker::new(Kind::Design, Some("new queue topology")).to_comment_body();
        assert!(b.contains("**CR kind: Design review** - new queue topology"));
        assert!(b.contains(SEARCH_TOKEN));
    }

    #[test]
    fn why_cannot_escape_the_html_comment() {
        let evil = "--> <img src=x onerror=alert(1)> <!-- & stuff";
        let m = Marker::new(Kind::Ai, Some(evil));
        let line = m.to_marker_line();
        assert_eq!(line.matches("-->").count(), 1, "{line}");
        assert!(line.ends_with("-->"));
        assert!(!line[MARKER_PREFIX.len()..].contains('<'));
        assert_eq!(parse(&line).unwrap().why.as_deref(), Some(evil));
        let human = m.to_comment_body();
        assert!(!human.lines().nth(1).unwrap().contains("<img"));
    }

    #[test]
    fn why_is_collapsed_and_capped() {
        let m = Marker::new(Kind::Ai, Some("  two\n\nlines   here "));
        assert_eq!(m.why.as_deref(), Some("two lines here"));
        let long = "x".repeat(1000);
        let m = Marker::new(Kind::Ai, Some(&long));
        assert_eq!(m.why.unwrap().chars().count(), MAX_WHY_CHARS);
        assert_eq!(Marker::new(Kind::Ai, Some("   ")).why, None);
    }

    #[test]
    fn parse_tolerates_surrounding_text_and_spacing() {
        let b = "hey team\n<!--   pr-begger:   {\"kind\":\"ai\",\"v\":1}-->\nthanks";
        // prefix must match exactly; extra spaces inside the prefix are not ours
        assert_eq!(parse(b), None);
        let b = "hey team\n<!-- pr-begger:{\"kind\":\"ai\",\"v\":1}   -->\nthanks";
        assert_eq!(parse(b).unwrap().kind, Kind::Ai);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert_eq!(parse("no marker here"), None);
        assert_eq!(
            parse("<!-- pr-begger: {\"v\":1,\"kind\":\"lgtm\"} -->"),
            None
        );
        assert_eq!(parse("<!-- pr-begger: {\"v\":1,\"kind\":\"ai\""), None);
        assert_eq!(parse("<!-- pr-begger: not json -->"), None);
        assert_eq!(parse("<!-- pr-begger: {\"v\":0,\"kind\":\"ai\"} -->"), None);
    }

    #[test]
    fn parse_accepts_future_versions_and_fields() {
        let b = "<!-- pr-begger: {\"v\":2,\"kind\":\"design\",\"extra\":true} -->";
        assert_eq!(parse(b).unwrap().kind, Kind::Design);
    }

    #[test]
    fn newest_marker_wins() {
        let comments = vec![
            comment("1", "alice", &body(Kind::RubberStamp), 1, None),
            comment("2", "bob", "lgtm", 5, None),
            comment("3", "carol", &body(Kind::Design), 3, None),
        ];
        let found = newest(&comments).unwrap();
        assert_eq!(found.comment.id, "3");
        assert_eq!(found.marker.kind, Kind::Design);
    }

    #[test]
    fn an_edit_makes_an_old_comment_newest() {
        let comments = vec![
            comment("1", "alice", &body(Kind::Ai), 1, Some(9)),
            comment("2", "carol", &body(Kind::Design), 3, None),
        ];
        assert_eq!(newest(&comments).unwrap().marker.kind, Kind::Ai);
    }

    #[test]
    fn ties_go_to_the_later_comment() {
        let comments = vec![
            comment("1", "alice", &body(Kind::Ai), 2, None),
            comment("2", "carol", &body(Kind::Design), 2, None),
        ];
        assert_eq!(newest(&comments).unwrap().comment.id, "2");
    }

    #[test]
    fn unreadable_markers_do_not_win() {
        let comments = vec![
            comment("1", "alice", &body(Kind::Ai), 1, None),
            comment(
                "2",
                "carol",
                "<!-- pr-begger: {\"v\":1,\"kind\":\"??\"} -->",
                4,
                None,
            ),
        ];
        assert_eq!(newest(&comments).unwrap().comment.id, "1");
        assert_eq!(newest(&[comment("x", "a", "plain", 1, None)]), None);
    }

    #[test]
    fn re_request_edits_own_newest_marker() {
        let comments = vec![
            comment("1", "Saar", &body(Kind::Ai), 1, None),
            comment("2", "bob", &body(Kind::Design), 2, None),
            comment("3", "saar", &body(Kind::RubberStamp), 3, None),
            comment("4", "saar", "unrelated", 4, None),
        ];
        assert_eq!(
            plan_request(&comments, "saar"),
            Plan::Update {
                comment_id: "3".into()
            }
        );
        assert_eq!(plan_request(&comments, "dave"), Plan::Create);
        assert_eq!(plan_request(&[], "saar"), Plan::Create);
    }
}
