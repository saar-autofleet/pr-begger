//! People and teams you follow, kept in `follow.toml` next to the config.
//!
//! ```toml
//! users = ["omril1", "galinareznikk"]
//! teams = ["Autofleet/dev-ordering"]
//! ```
//!
//! Edited by `pr-begger follow` / `unfollow` and the TUI, or by hand.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    User(String),
    Team { org: String, slug: String },
}

impl fmt::Display for Who {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Who::User(u) => f.write_str(u),
            Who::Team { org, slug } => write!(f, "{org}/{slug}"),
        }
    }
}

fn valid(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

impl Who {
    /// Accepts `login`, `@login`, a profile URL, `org/team`, `@org/team`,
    /// `teams/team` (in `default_org`), or a team URL
    /// (`https://github.com/orgs/<org>/teams/<team>`).
    pub fn parse(s: &str, default_org: Option<&str>) -> Result<Who> {
        let raw = s.trim();
        let s = raw
            .strip_prefix("https://")
            .or_else(|| raw.strip_prefix("http://"))
            .unwrap_or(raw);
        let s = s.strip_prefix("www.").unwrap_or(s);
        let s = s.strip_prefix("github.com/").unwrap_or(s);
        let s = s.trim_start_matches('@').trim_end_matches('/');
        let parts: Vec<&str> = s.split('/').collect();
        let who = match parts.as_slice() {
            [user] => Who::User(user.to_string()),
            ["orgs", org, "teams", slug] => Who::Team {
                org: org.to_string(),
                slug: slug.to_string(),
            },
            ["teams", slug] => match default_org {
                Some(org) => Who::Team {
                    org: org.to_string(),
                    slug: slug.to_string(),
                },
                None => bail!("`{raw}`: say which org, like Autofleet/{slug}"),
            },
            [org, slug] => Who::Team {
                org: org.to_string(),
                slug: slug.to_string(),
            },
            _ => bail!("`{raw}` is not a user or org/team"),
        };
        let ok = match &who {
            Who::User(u) => valid(u),
            Who::Team { org, slug } => valid(org) && valid(slug),
        };
        if !ok {
            bail!("`{raw}` is not a user or org/team");
        }
        Ok(who)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Follows {
    pub users: Vec<String>,
    pub teams: Vec<String>,
}

impl Follows {
    pub fn is_empty(&self) -> bool {
        self.users.is_empty() && self.teams.is_empty()
    }

    fn list(&mut self, who: &Who) -> (&mut Vec<String>, String) {
        match who {
            Who::User(_) => (&mut self.users, who.to_string()),
            Who::Team { .. } => (&mut self.teams, who.to_string()),
        }
    }

    /// Add; returns false if already followed (case-insensitively).
    pub fn add(&mut self, who: &Who) -> bool {
        let (list, name) = self.list(who);
        if list.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            return false;
        }
        list.push(name);
        list.sort_by_key(|s| s.to_lowercase());
        true
    }

    /// Remove; returns false if it was not followed.
    pub fn remove(&mut self, who: &Who) -> bool {
        let (list, name) = self.list(who);
        let before = list.len();
        list.retain(|x| !x.eq_ignore_ascii_case(&name));
        list.len() != before
    }

    /// Everything followed, users first.
    pub fn all(&self) -> Vec<Who> {
        let users = self.users.iter().map(|u| Who::User(u.clone()));
        let teams = self.teams.iter().filter_map(|t| {
            let (org, slug) = t.split_once('/')?;
            Some(Who::Team {
                org: org.into(),
                slug: slug.into(),
            })
        });
        users.chain(teams).collect()
    }
}

pub struct FollowFile {
    path: Option<PathBuf>,
}

impl FollowFile {
    pub fn new(path: Option<PathBuf>) -> Self {
        FollowFile { path }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn load(&self) -> Result<Follows> {
        let Some(path) = &self.path else {
            return Ok(Follows::default());
        };
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Follows::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self, follows: &Follows) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = format!(
            "# People and teams pr-begger follows. Edit freely, or use\n# `pr-begger follow` / `unfollow`, or `p` in the TUI.\n{}",
            toml::to_string(follows)?
        );
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }
}

/// Split authors into groups small enough for one GitHub search query
/// (queries are capped at 256 characters and 5 boolean operators).
pub fn author_chunks(authors: &[String]) -> Vec<Vec<String>> {
    authors.chunks(5).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team(o: &str, s: &str) -> Who {
        Who::Team {
            org: o.into(),
            slug: s.into(),
        }
    }

    #[test]
    fn parses_users_and_teams() {
        let org = Some("Autofleet");
        assert_eq!(
            Who::parse("omril1", org).unwrap(),
            Who::User("omril1".into())
        );
        assert_eq!(
            Who::parse("@omril1", org).unwrap(),
            Who::User("omril1".into())
        );
        assert_eq!(
            Who::parse("https://github.com/omril1", org).unwrap(),
            Who::User("omril1".into())
        );
        assert_eq!(
            Who::parse("teams/dev-ordering", org).unwrap(),
            team("Autofleet", "dev-ordering")
        );
        assert_eq!(
            Who::parse("@Autofleet/dev-ordering", None).unwrap(),
            team("Autofleet", "dev-ordering")
        );
        assert_eq!(
            Who::parse("https://github.com/orgs/Autofleet/teams/dev-ordering", None).unwrap(),
            team("Autofleet", "dev-ordering")
        );
        assert!(Who::parse("teams/x", None).is_err());
        assert!(Who::parse("a b", org).is_err());
        assert!(Who::parse("", org).is_err());
        assert!(Who::parse("a/b/c", org).is_err());
    }

    #[test]
    fn add_and_remove_are_case_insensitive_and_sorted() {
        let mut f = Follows::default();
        assert!(f.add(&Who::User("Zed".into())));
        assert!(f.add(&Who::User("amy".into())));
        assert!(!f.add(&Who::User("zed".into())));
        assert_eq!(f.users, vec!["amy", "Zed"]);
        assert!(f.add(&team("Autofleet", "dev-ordering")));
        assert_eq!(f.all().len(), 3);
        assert!(f.remove(&Who::User("ZED".into())));
        assert!(!f.remove(&Who::User("zed".into())));
        assert!(f.remove(&team("autofleet", "DEV-ORDERING")));
        assert_eq!(f.all(), vec![Who::User("amy".into())]);
    }

    #[test]
    fn file_round_trips() {
        let dir = std::env::temp_dir().join(format!("pr-begger-follow-{}", std::process::id()));
        let file = FollowFile::new(Some(dir.join("follow.toml")));
        assert_eq!(file.load().unwrap(), Follows::default());
        let mut f = Follows::default();
        f.add(&Who::User("omril1".into()));
        f.add(&team("Autofleet", "dev-ordering"));
        file.save(&f).unwrap();
        assert_eq!(file.load().unwrap(), f);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chunks_authors() {
        let a: Vec<String> = (0..12).map(|i| format!("u{i}")).collect();
        let c = author_chunks(&a);
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|c| c.len() <= 5));
    }
}
