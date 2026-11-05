//! Small JSON files: the last board (so startup draws instantly) and when
//! you last looked at each of your PRs (so new activity can be flagged).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::model::Pr;

const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub scope: String,
    pub board: Vec<Pr>,
    pub board_at: Option<DateTime<Utc>>,
    pub mine: Vec<Pr>,
    pub mine_at: Option<DateTime<Utc>>,
}

fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write through a temp file so a crash never leaves half a cache behind.
/// Failures are ignored: these files are conveniences.
fn write<T: Serialize>(path: &Path, value: &T) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_string(value) else {
        return;
    };
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

pub struct Cache {
    path: Option<PathBuf>,
}

impl Cache {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Cache {
            path: dir.map(|d| d.join("board.json")),
        }
    }

    /// The cached snapshot, only if it was built for this exact scope.
    pub fn load(&self, scope: &str) -> Option<Snapshot> {
        let snap: Snapshot = read(self.path.as_ref()?)?;
        (snap.version == CACHE_VERSION && snap.scope == scope).then_some(snap)
    }

    pub fn save(&self, snap: &Snapshot) {
        if let Some(p) = &self.path {
            let mut snap = snap.clone();
            snap.version = CACHE_VERSION;
            write(p, &snap);
        }
    }
}

/// When you last looked at each of your PRs, by `owner/repo#n`.
pub struct Seen {
    path: Option<PathBuf>,
    map: HashMap<String, DateTime<Utc>>,
}

impl Seen {
    pub fn load(dir: Option<PathBuf>) -> Self {
        let path = dir.map(|d| d.join("seen.json"));
        let map = path.as_deref().and_then(read).unwrap_or_default();
        Seen { path, map }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Seen {
            path: None,
            map: HashMap::new(),
        }
    }

    /// Activity counts as new when it is later than the last look. A PR
    /// never looked at gets a baseline of "now" the first time it is seen,
    /// so the first run does not flag every old comment as news.
    pub fn is_new(&mut self, pr: &Pr, now: DateTime<Utc>) -> bool {
        let Some(last) = pr.activity.as_ref().and_then(|a| a.last_by_others) else {
            return false;
        };
        match self.map.get(&pr.key()) {
            Some(seen) => last > *seen,
            None => {
                self.map.insert(pr.key(), now);
                false
            }
        }
    }

    pub fn mark(&mut self, pr: &Pr, now: DateTime<Utc>) {
        self.map.insert(pr.key(), now);
    }

    /// Forget PRs that are no longer open so the file does not grow forever.
    pub fn retain_open(&mut self, open: &[Pr]) {
        let keys: std::collections::HashSet<String> = open.iter().map(Pr::key).collect();
        self.map.retain(|k, _| keys.contains(k));
    }

    pub fn save(&self) {
        if let Some(p) = &self.path {
            write(p, &self.map);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::parse_search_response;
    use chrono::Duration;

    fn sample() -> Vec<Pr> {
        parse_search_response(include_str!("../tests/fixtures/search.json")).unwrap()
    }

    #[test]
    fn seen_baselines_then_flags_newer_activity() {
        let pr = sample().pop().unwrap();
        let activity_at = pr.activity.as_ref().unwrap().last_by_others.unwrap();
        let mut seen = Seen::in_memory();
        // first sight: baseline, not news
        assert!(!seen.is_new(&pr, activity_at + Duration::hours(1)));
        assert!(!seen.is_new(&pr, activity_at + Duration::hours(2)));
        // looked before the activity happened: news
        seen.mark(&pr, activity_at - Duration::hours(1));
        assert!(seen.is_new(&pr, activity_at));
        // looked again after: quiet
        seen.mark(&pr, activity_at + Duration::minutes(1));
        assert!(!seen.is_new(&pr, activity_at));
    }

    #[test]
    fn cache_round_trips_and_is_scoped() {
        let dir = std::env::temp_dir().join(format!("pr-begger-test-{}", std::process::id()));
        let cache = Cache::new(Some(dir.clone()));
        let snap = Snapshot {
            scope: "org:A".into(),
            board: sample(),
            board_at: Some(Utc::now()),
            ..Default::default()
        };
        cache.save(&snap);
        let back = cache.load("org:A").unwrap();
        assert_eq!(back.board, snap.board);
        assert!(cache.load("org:B").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
