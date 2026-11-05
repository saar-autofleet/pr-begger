//! The three kinds of code review a PR can beg for.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// What kind of review a PR needs. Ordered by how much human attention it
/// costs, which is also the order the board shows them in.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Mechanical change: approve as-is.
    #[value(alias = "rs", alias = "stamp")]
    RubberStamp,
    /// Let an AI agent review it.
    #[value(alias = "agent", alias = "bot")]
    Ai,
    /// Needs a human design review.
    #[value(alias = "human", alias = "arch")]
    Design,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::RubberStamp, Kind::Ai, Kind::Design];

    /// The machine id written into the marker (`rubber-stamp`, `ai`, `design`).
    pub fn id(self) -> &'static str {
        match self {
            Kind::RubberStamp => "rubber-stamp",
            Kind::Ai => "ai",
            Kind::Design => "design",
        }
    }

    /// Human label used in comments and the TUI.
    pub fn label(self) -> &'static str {
        match self {
            Kind::RubberStamp => "Rubber stamp",
            Kind::Ai => "AI review",
            Kind::Design => "Design review",
        }
    }

    /// Short fixed-width (5 cell) tag for list rows.
    pub fn tag(self) -> &'static str {
        match self {
            Kind::RubberStamp => "STAMP",
            Kind::Ai => " A.I ",
            Kind::Design => "DSIGN",
        }
    }

    /// One-line explanation for the comment and the help screen.
    pub fn blurb(self) -> &'static str {
        match self {
            Kind::RubberStamp => "mechanical change, approve as-is",
            Kind::Ai => "let an AI agent go over it",
            Kind::Design => "needs a human design review",
        }
    }

    pub fn emoji(self) -> &'static str {
        match self {
            Kind::RubberStamp => "🟩",
            Kind::Ai => "🤖",
            Kind::Design => "🧭",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for Kind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rubber-stamp" | "rubber_stamp" | "rubberstamp" | "rs" | "stamp" => {
                Ok(Kind::RubberStamp)
            }
            "ai" | "agent" | "bot" => Ok(Kind::Ai),
            "design" | "human" | "arch" => Ok(Kind::Design),
            other => Err(format!(
                "unknown CR kind `{other}` (expected rubber-stamp, ai or design)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for k in Kind::ALL {
            assert_eq!(k.id().parse::<Kind>().unwrap(), k);
        }
    }

    #[test]
    fn aliases_parse() {
        assert_eq!("RS".parse::<Kind>().unwrap(), Kind::RubberStamp);
        assert_eq!("agent".parse::<Kind>().unwrap(), Kind::Ai);
        assert_eq!(" Design ".parse::<Kind>().unwrap(), Kind::Design);
        assert!("lgtm".parse::<Kind>().is_err());
    }

    #[test]
    fn tags_are_fixed_width() {
        for k in Kind::ALL {
            assert_eq!(k.tag().chars().count(), 5);
        }
    }
}
