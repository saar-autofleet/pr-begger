//! Parsing a PR reference from the command line.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

impl fmt::Display for PrRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}#{}", self.owner, self.repo, self.number)
    }
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl FromStr for PrRef {
    type Err = String;

    /// Accepts `owner/repo#123` or a GitHub PR URL
    /// (`https://github.com/owner/repo/pull/123`, with or without scheme and
    /// trailing path like `/files`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || format!("`{s}` is not a PR reference (use a PR URL or owner/repo#number)");
        let s = s.trim();
        let is_url = s.contains("github.com/") || s.contains("://");
        let (owner, repo, num) = if !is_url {
            let (path, num) = s.split_once('#').ok_or_else(bad)?;
            let (owner, repo) = path.split_once('/').ok_or_else(bad)?;
            (owner, repo, num)
        } else {
            let rest = s
                .strip_prefix("https://")
                .or_else(|| s.strip_prefix("http://"))
                .unwrap_or(s);
            let rest = rest.strip_prefix("www.").unwrap_or(rest);
            let rest = rest.strip_prefix("github.com/").ok_or_else(bad)?;
            let mut parts = rest.split('/');
            let owner = parts.next().ok_or_else(bad)?;
            let repo = parts.next().ok_or_else(bad)?;
            match parts.next() {
                Some("pull") | Some("pulls") => {}
                _ => return Err(bad()),
            }
            let num = parts.next().ok_or_else(bad)?;
            let num = num.split(['?', '#']).next().unwrap_or(num);
            (owner, repo, num)
        };
        if !valid_name(owner) || !valid_name(repo) {
            return Err(bad());
        }
        let number = num.parse::<u64>().map_err(|_| bad())?;
        if number == 0 {
            return Err(bad());
        }
        Ok(PrRef {
            owner: owner.to_string(),
            repo: repo.to_string(),
            number,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(o: &str, n: &str, num: u64) -> PrRef {
        PrRef {
            owner: o.into(),
            repo: n.into(),
            number: num,
        }
    }

    #[test]
    fn parses_short_form() {
        assert_eq!("Autofleet/api#42".parse(), Ok(r("Autofleet", "api", 42)));
    }

    #[test]
    fn parses_urls() {
        for s in [
            "https://github.com/saar-autofleet/pr-begger/pull/7",
            "https://github.com/saar-autofleet/pr-begger/pull/7/files",
            "github.com/saar-autofleet/pr-begger/pull/7",
            "https://github.com/saar-autofleet/pr-begger/pull/7#issuecomment-1",
            "https://github.com/saar-autofleet/pr-begger/pull/7?w=1",
        ] {
            assert_eq!(s.parse(), Ok(r("saar-autofleet", "pr-begger", 7)), "{s}");
        }
    }

    #[test]
    fn rejects_junk() {
        for s in [
            "",
            "42",
            "owner#1",
            "o/r#x",
            "o/r#0",
            "https://gitlab.com/o/r/pull/1",
            "https://github.com/o/r/issues/1",
            "o/r r#1",
        ] {
            assert!(s.parse::<PrRef>().is_err(), "{s}");
        }
    }

    #[test]
    fn displays_short_form() {
        assert_eq!(r("o", "r", 3).to_string(), "o/r#3");
    }
}
