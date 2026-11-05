//! Config file and where pr-begger keeps its files.
//!
//! `$PR_BEGGER_CONFIG`, else `$XDG_CONFIG_HOME/pr-begger/config.toml`, else
//! `~/.config/pr-begger/config.toml`. Missing file means defaults.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub const DEFAULT_ORG: &str = "Autofleet";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Organisations whose open PRs are searched.
    pub orgs: Vec<String>,
    /// Extra individual repos (`owner/name`), e.g. outside the orgs.
    pub repos: Vec<String>,
    /// Auto-refresh period in seconds while the TUI is open. 0 disables.
    pub refresh_secs: u64,
    /// Upper bound on PRs fetched per search scope.
    pub max_prs: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            orgs: vec![DEFAULT_ORG.to_string()],
            repos: Vec::new(),
            refresh_secs: 180,
            max_prs: 200,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Self> {
        let cfg: Config = toml::from_str(text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                Self::parse(&text).with_context(|| format!("reading config {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading config {}", path.display())),
        }
    }

    /// Command-line scope replaces the file's scope entirely, so
    /// `--repo me/x` does not silently also search the default org.
    pub fn with_overrides(mut self, orgs: &[String], repos: &[String]) -> Result<Self> {
        if !orgs.is_empty() || !repos.is_empty() {
            self.orgs = orgs.to_vec();
            self.repos = repos.to_vec();
        }
        self.validate()?;
        Ok(self)
    }

    fn validate(&self) -> Result<()> {
        if self.orgs.is_empty() && self.repos.is_empty() {
            bail!("nothing to search: configure at least one org or repo");
        }
        for org in &self.orgs {
            if org.is_empty() || org.contains('/') || org.contains(char::is_whitespace) {
                bail!("`{org}` is not an org name");
            }
        }
        for repo in &self.repos {
            let ok = repo
                .split_once('/')
                .is_some_and(|(o, n)| !o.is_empty() && !n.is_empty() && !n.contains('/'))
                && !repo.contains(char::is_whitespace);
            if !ok {
                bail!("`{repo}` is not an owner/name repo");
            }
        }
        if !(1..=1000).contains(&self.max_prs) {
            bail!("max_prs must be between 1 and 1000");
        }
        Ok(())
    }

    /// GitHub search qualifiers, one per scope. Each becomes its own search,
    /// run in parallel: a single query ORing them would share one result cap.
    pub fn scopes(&self) -> Vec<String> {
        self.orgs
            .iter()
            .map(|o| format!("org:{o}"))
            .chain(self.repos.iter().map(|r| format!("repo:{r}")))
            .collect()
    }

    /// Stable key for the cache, so a cache built for one scope is never
    /// shown for another.
    pub fn scope_key(&self) -> String {
        self.scopes().join(" ")
    }

    pub fn refresh(&self) -> Option<Duration> {
        (self.refresh_secs > 0).then(|| Duration::from_secs(self.refresh_secs.max(30)))
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn xdg(var: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home().map(|h| h.join(fallback)))
        .map(|p| p.join("pr-begger"))
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PR_BEGGER_CONFIG") {
        return Some(PathBuf::from(p));
    }
    xdg("XDG_CONFIG_HOME", ".config").map(|d| d.join("config.toml"))
}

/// `follow.toml` lives next to the config file.
pub fn follow_path(config: Option<&Path>) -> Option<PathBuf> {
    config
        .and_then(Path::parent)
        .map(|d| d.join("follow.toml"))
        .or_else(|| xdg("XDG_CONFIG_HOME", ".config").map(|d| d.join("follow.toml")))
}

pub fn cache_dir() -> Option<PathBuf> {
    xdg("XDG_CACHE_HOME", ".cache")
}

pub fn state_dir() -> Option<PathBuf> {
    xdg("XDG_STATE_HOME", ".local/state")
}

pub const EXAMPLE: &str = r#"# ~/.config/pr-begger/config.toml
orgs = ["Autofleet"]          # orgs whose open PRs are searched
repos = []                    # extra owner/name repos
refresh_secs = 180            # auto-refresh while the TUI is open (0 = off)
max_prs = 200                 # cap per org/repo search
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_autofleet() {
        let c = Config::default();
        assert_eq!(c.scopes(), vec!["org:Autofleet"]);
        assert_eq!(Config::parse("").unwrap(), c);
    }

    #[test]
    fn example_parses() {
        Config::parse(EXAMPLE).unwrap();
    }

    #[test]
    fn parses_scopes() {
        let c =
            Config::parse("orgs = [\"a\", \"b\"]\nrepos = [\"me/x\"]\nrefresh_secs = 0").unwrap();
        assert_eq!(c.scopes(), vec!["org:a", "org:b", "repo:me/x"]);
        assert_eq!(c.refresh(), None);
    }

    #[test]
    fn rejects_unknown_and_bad_values() {
        assert!(Config::parse("org = \"a\"").is_err());
        assert!(Config::parse("repos = [\"noslash\"]").is_err());
        assert!(Config::parse("orgs = [\"a/b\"]").is_err());
        assert!(Config::parse("orgs = []").is_err());
        assert!(Config::parse("max_prs = 0").is_err());
    }

    #[test]
    fn flags_replace_file_scope() {
        let c = Config::default()
            .with_overrides(&[], &["saar-autofleet/pr-begger".into()])
            .unwrap();
        assert_eq!(c.scopes(), vec!["repo:saar-autofleet/pr-begger"]);
        let c = Config::default().with_overrides(&[], &[]).unwrap();
        assert_eq!(c.scopes(), vec!["org:Autofleet"]);
    }

    #[test]
    fn refresh_has_a_floor() {
        let c = Config::parse("refresh_secs = 1").unwrap();
        assert_eq!(c.refresh(), Some(Duration::from_secs(30)));
    }
}
