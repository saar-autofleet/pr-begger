//! The interactive board.

mod fzf;
mod pixel;
mod view;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::config::{self, Config};
use crate::follow::{FollowFile, Follows, Who};
use crate::github::Client;
use crate::kind::Kind;
use crate::model::{Pr, Request};
use crate::store::{Cache, Seen, Snapshot};
use crate::target::PrRef;

/// A PR we just begged for stays on the board this long even if GitHub's
/// search index has not caught up with the new comment yet.
const POST_GRACE: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Board,
    Mine,
    Following,
}

impl Screen {
    pub const ALL: [Screen; 3] = [Screen::Board, Screen::Mine, Screen::Following];

    pub fn idx(self) -> usize {
        match self {
            Screen::Board => 0,
            Screen::Mine => 1,
            Screen::Following => 2,
        }
    }
}

pub enum Popup {
    Help,
    Pick(Box<Picker>),
    Follows(FollowEditor),
}

/// The `p` window: who you follow, with add/remove.
#[derive(Default)]
pub struct FollowEditor {
    pub cursor: usize,
    /// `Some` while typing a new user or team.
    pub input: Option<String>,
    pub checking: bool,
}

pub struct Picker {
    pub pr: Pr,
    pub choice: usize,
    pub why: String,
}

pub enum Msg {
    Board(Result<Vec<Pr>, String>),
    Mine(Result<Vec<Pr>, String>),
    Following(Result<(Vec<Pr>, Vec<String>), String>),
    /// A user/team typed into the follow editor, checked against GitHub.
    FollowChecked(Result<(Who, String), String>),
    Posted {
        pr: Box<Pr>,
        kind: Kind,
        why: Option<String>,
        result: Result<(String, bool), String>,
    },
}

pub struct Status {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

pub struct App {
    cfg: Config,
    client: Option<Arc<Client>>,
    tx: Sender<Msg>,
    cache: Cache,
    pub seen: Seen,
    pub screen: Screen,
    pub board: Vec<Pr>,
    pub mine: Vec<Pr>,
    pub board_at: Option<DateTime<Utc>>,
    pub mine_at: Option<DateTime<Utc>>,
    pub loading_board: bool,
    pub loading_mine: bool,
    pub follow_file: FollowFile,
    pub follows: Follows,
    pub following: Vec<Pr>,
    pub following_at: Option<DateTime<Utc>>,
    pub loading_following: bool,
    /// Lowercased logins of followed people and followed teams' members.
    pub followed_authors: HashSet<String>,
    /// Board shows only PRs by followed authors.
    pub only_followed: bool,
    last_fetch: Instant,
    pub filter: Option<Kind>,
    pub cursor: [usize; 3],
    pub scroll: [usize; 3],
    pub popup: Option<Popup>,
    pub new_keys: HashSet<String>,
    posted: HashMap<String, (Pr, Instant)>,
    pub status: Option<Status>,
    pub tick: u64,
    pub quit: bool,
    /// `/` was pressed: the run loop hands the terminal to fzf.
    want_search: bool,
    /// Following screen grouped by repo instead of by author.
    pub following_by_repo: bool,
}

/// The Following screen's group for a PR, lowercased for sorting.
pub fn following_group(pr: &Pr, by_repo: bool) -> String {
    if by_repo {
        pr.repo.to_lowercase()
    } else {
        pr.author.to_lowercase()
    }
}

/// Board order: by kind (cheapest review first), then oldest first, since the
/// longest-waiting beggar should be served first. Unknown kinds go last.
pub fn sort_prs(prs: &mut [Pr]) {
    prs.sort_by(|a, b| {
        let ka = a.kind().map_or(9, |k| k as u8);
        let kb = b.kind().map_or(9, |k| k as u8);
        ka.cmp(&kb)
            .then(a.created_at.cmp(&b.created_at))
            .then(a.key().cmp(&b.key()))
    });
}

impl App {
    fn new(
        cfg: Config,
        client: Option<Arc<Client>>,
        tx: Sender<Msg>,
        cache: Cache,
        seen: Seen,
        follow_file: FollowFile,
    ) -> Self {
        let (follows, follow_err) = match follow_file.load() {
            Ok(f) => (f, None),
            Err(e) => (Follows::default(), Some(format!("{e:#}"))),
        };
        let mut app = App {
            cfg,
            client,
            tx,
            cache,
            seen,
            screen: Screen::Board,
            board: Vec::new(),
            mine: Vec::new(),
            board_at: None,
            mine_at: None,
            loading_board: false,
            loading_mine: false,
            follow_file,
            follows,
            following: Vec::new(),
            following_at: None,
            loading_following: false,
            followed_authors: HashSet::new(),
            only_followed: false,
            last_fetch: Instant::now(),
            filter: None,
            cursor: [0; 3],
            scroll: [0; 3],
            popup: None,
            new_keys: HashSet::new(),
            posted: HashMap::new(),
            status: None,
            tick: 0,
            quit: false,
            want_search: false,
            following_by_repo: false,
        };
        if let Some(snap) = app.cache.load(&app.cfg.scope_key()) {
            app.board = snap.board;
            app.board_at = snap.board_at;
            app.set_mine(snap.mine);
            app.mine_at = snap.mine_at;
            sort_prs(&mut app.board);
            app.followed_authors = snap.followed_authors.into_iter().collect();
            app.set_following(snap.following);
            app.following_at = snap.following_at;
        }
        if let Some(e) = follow_err {
            app.say(format!("follow list: {e}"), true);
        }
        app
    }

    pub fn scope_label(&self) -> String {
        self.cfg.scopes().join(" ")
    }

    pub fn loading(&self) -> bool {
        self.loading_board || self.loading_mine || self.loading_following
    }

    fn say(&mut self, text: impl Into<String>, error: bool) {
        self.status = Some(Status {
            text: text.into(),
            error,
            at: Instant::now(),
        });
    }

    fn refresh(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.last_fetch = Instant::now();
        if !self.loading_board {
            self.loading_board = true;
            let (c, cfg, tx) = (client.clone(), self.cfg.clone(), self.tx.clone());
            std::thread::spawn(move || {
                let r = c.fetch_board(&cfg).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Msg::Board(r));
            });
        }
        if !self.loading_mine {
            self.loading_mine = true;
            let (c, cfg, tx) = (client.clone(), self.cfg.clone(), self.tx.clone());
            std::thread::spawn(move || {
                let r = c.fetch_mine(&cfg).map_err(|e| format!("{e:#}"));
                let _ = tx.send(Msg::Mine(r));
            });
        }
        self.refresh_following();
    }

    fn refresh_following(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        if self.loading_following {
            return;
        }
        if self.follows.is_empty() {
            self.following.clear();
            self.followed_authors.clear();
            self.following_at = Some(Utc::now());
            return;
        }
        self.loading_following = true;
        let (cfg, follows, tx) = (self.cfg.clone(), self.follows.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let r = client
                .fetch_following(&cfg, &follows)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Following(r));
        });
    }

    /// Following order: grouped by author or by repo (alphabetical),
    /// newest first within a group.
    fn set_following(&mut self, mut prs: Vec<Pr>) {
        let by_repo = self.following_by_repo;
        prs.sort_by(|a, b| {
            following_group(a, by_repo)
                .cmp(&following_group(b, by_repo))
                .then(b.created_at.cmp(&a.created_at))
        });
        self.following = prs;
    }

    fn toggle_following_grouping(&mut self) {
        let key = self.selected().map(Pr::key);
        self.following_by_repo = !self.following_by_repo;
        let prs = std::mem::take(&mut self.following);
        self.set_following(prs);
        // keep the cursor on the same PR
        if let Some(k) = key {
            if let Some(i) = self.items().iter().position(|p| p.key() == k) {
                self.cursor[Screen::Following.idx()] = i;
            }
        }
        let by = if self.following_by_repo {
            "repo"
        } else {
            "author"
        };
        self.say(format!("following grouped by {by}"), false);
    }

    fn save_follows(&mut self) {
        if let Err(e) = self.follow_file.save(&self.follows) {
            self.say(format!("could not save follow list: {e:#}"), true);
        }
    }

    fn follow(&mut self, who: Who, desc: &str) {
        if self.follows.add(&who) {
            self.save_follows();
            self.say(format!("following {desc}"), false);
            self.loading_following = false;
            self.refresh_following();
        } else {
            self.say(format!("already following {who}"), false);
        }
    }

    fn unfollow(&mut self, who: &Who) {
        if self.follows.remove(who) {
            self.save_follows();
            self.say(format!("unfollowed {who}"), false);
            self.refresh_following();
        }
    }

    fn set_mine(&mut self, mut mine: Vec<Pr>) {
        let now = Utc::now();
        self.new_keys = mine
            .iter()
            .filter(|p| self.seen.is_new(p, now))
            .map(Pr::key)
            .collect();
        mine.sort_by(|a, b| {
            let na = self.new_keys.contains(&a.key());
            let nb = self.new_keys.contains(&b.key());
            nb.cmp(&na).then(b.updated_at.cmp(&a.updated_at))
        });
        self.mine = mine;
    }

    fn snapshot(&self) {
        self.cache.save(&Snapshot {
            version: 0,
            scope: self.cfg.scope_key(),
            board: self.board.clone(),
            board_at: self.board_at,
            mine: self.mine.clone(),
            mine_at: self.mine_at,
            following: self.following.clone(),
            following_at: self.following_at,
            followed_authors: self.followed_authors.iter().cloned().collect(),
        });
    }

    fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Board(Ok(mut prs)) => {
                self.loading_board = false;
                self.posted.retain(|_, (_, at)| at.elapsed() < POST_GRACE);
                for (key, (pr, _)) in &self.posted {
                    match prs.iter_mut().find(|p| &p.key() == key) {
                        // fetched copy predates our comment: keep our kind
                        Some(p)
                            if p.request.as_ref().map(|r| r.at)
                                < pr.request.as_ref().map(|r| r.at) =>
                        {
                            p.request = pr.request.clone()
                        }
                        Some(_) => {}
                        None => prs.push(pr.clone()),
                    }
                }
                sort_prs(&mut prs);
                self.board = prs;
                self.board_at = Some(Utc::now());
                self.clamp();
                self.snapshot();
            }
            Msg::Mine(Ok(prs)) => {
                self.loading_mine = false;
                self.seen.retain_open(&prs);
                self.set_mine(prs);
                self.seen.save();
                self.mine_at = Some(Utc::now());
                self.clamp();
                self.snapshot();
            }
            Msg::Following(Ok((prs, authors))) => {
                self.loading_following = false;
                self.followed_authors = authors.iter().map(|a| a.to_lowercase()).collect();
                self.set_following(prs);
                self.following_at = Some(Utc::now());
                self.clamp();
                self.snapshot();
            }
            Msg::Following(Err(e)) => {
                self.loading_following = false;
                self.say(format!("following refresh failed: {e}"), true);
            }
            Msg::FollowChecked(r) => {
                if let Some(Popup::Follows(ed)) = &mut self.popup {
                    ed.checking = false;
                }
                match r {
                    Ok((who, desc)) => self.follow(who, &desc),
                    Err(e) => self.say(e, true),
                }
            }
            Msg::Board(Err(e)) => {
                self.loading_board = false;
                self.say(format!("board refresh failed: {e}"), true);
            }
            Msg::Mine(Err(e)) => {
                self.loading_mine = false;
                self.say(format!("my-PRs refresh failed: {e}"), true);
            }
            Msg::Posted {
                pr,
                kind,
                why,
                result,
            } => match result {
                Ok((_url, updated)) => {
                    let mut pr = *pr;
                    pr.request = Some(Request {
                        kind,
                        why,
                        by: None,
                        at: Utc::now(),
                    });
                    let key = pr.key();
                    for list in [&mut self.board, &mut self.mine] {
                        if let Some(p) = list.iter_mut().find(|p| p.key() == key) {
                            p.request = pr.request.clone();
                        }
                    }
                    if !self.board.iter().any(|p| p.key() == key) {
                        self.board.push(pr.clone());
                    }
                    sort_prs(&mut self.board);
                    self.posted.insert(key.clone(), (pr, Instant::now()));
                    let verb = if updated { "updated" } else { "posted" };
                    self.say(
                        format!("{} {verb}: {} on {key}", kind.emoji(), kind.label()),
                        false,
                    );
                    self.clamp();
                    self.snapshot();
                }
                Err(e) => self.say(format!("could not beg: {e}"), true),
            },
        }
    }

    /// The PRs the current screen lists, in display order.
    pub fn items(&self) -> Vec<&Pr> {
        let by_kind = |p: &&Pr| self.filter.is_none() || p.kind() == self.filter;
        match self.screen {
            Screen::Board => self
                .board
                .iter()
                .filter(by_kind)
                .filter(|p| {
                    !self.only_followed || self.followed_authors.contains(&p.author.to_lowercase())
                })
                .collect(),
            Screen::Mine => self.mine.iter().filter(by_kind).collect(),
            Screen::Following => self.following.iter().filter(by_kind).collect(),
        }
    }

    /// The list a screen draws from, before filters (for counts).
    pub fn list(&self, screen: Screen) -> &[Pr] {
        match screen {
            Screen::Board => &self.board,
            Screen::Mine => &self.mine,
            Screen::Following => &self.following,
        }
    }

    pub fn selected(&self) -> Option<&Pr> {
        self.items().get(self.cursor[self.screen.idx()]).copied()
    }

    pub fn cursor(&self) -> usize {
        self.cursor[self.screen.idx()]
    }

    fn clamp(&mut self) {
        for screen in Screen::ALL {
            let saved = self.screen;
            self.screen = screen;
            let n = self.items().len();
            let c = &mut self.cursor[screen.idx()];
            *c = (*c).min(n.saturating_sub(1));
            self.screen = saved;
        }
    }

    fn move_by(&mut self, delta: isize) {
        let n = self.items().len();
        if n == 0 {
            return;
        }
        let c = &mut self.cursor[self.screen.idx()];
        *c = (*c as isize + delta).clamp(0, n as isize - 1) as usize;
    }

    fn open_selected(&mut self) {
        let Some(pr) = self.selected().cloned() else {
            return;
        };
        match open_url(&pr.url) {
            Ok(()) => {
                if self.screen == Screen::Mine {
                    self.mark_seen(&pr);
                }
                self.say(format!("opened {}", pr.url), false);
            }
            Err(e) => self.say(format!("could not open browser: {e}"), true),
        }
    }

    fn mark_seen(&mut self, pr: &Pr) {
        self.seen.mark(pr, Utc::now());
        self.seen.save();
        self.new_keys.remove(&pr.key());
    }

    fn cycle_filter(&mut self, back: bool) {
        let order = [
            None,
            Some(Kind::RubberStamp),
            Some(Kind::Ai),
            Some(Kind::Design),
        ];
        let i = order.iter().position(|k| *k == self.filter).unwrap_or(0);
        let next = if back {
            (i + order.len() - 1) % order.len()
        } else {
            (i + 1) % order.len()
        };
        self.filter = order[next];
        self.cursor = [0; 3];
        self.scroll = [0; 3];
    }

    fn start_pick(&mut self) {
        let Some(pr) = self.selected().cloned() else {
            return;
        };
        if self.client.is_none() {
            return self.say("no GitHub token: cannot post", true);
        }
        let choice = pr
            .kind()
            .and_then(|k| Kind::ALL.iter().position(|x| *x == k))
            .unwrap_or(0);
        let why = pr
            .request
            .as_ref()
            .and_then(|r| r.why.clone())
            .unwrap_or_default();
        self.popup = Some(Popup::Pick(Box::new(Picker { pr, choice, why })));
    }

    fn submit_pick(&mut self, picker: Picker) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let kind = Kind::ALL[picker.choice];
        let why = Some(picker.why.trim().to_string()).filter(|w| !w.is_empty());
        let pr = picker.pr;
        let Some((owner, repo)) = pr.repo.split_once('/') else {
            return;
        };
        let target = PrRef {
            owner: owner.into(),
            repo: repo.into(),
            number: pr.number,
        };
        let tx = self.tx.clone();
        self.say(
            format!("begging for {} on {}…", kind.label(), pr.key()),
            false,
        );
        std::thread::spawn(move || {
            let result = crate::request::request(&client, &target, kind, why.as_deref())
                .map(|o| (o.url, o.updated))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Posted {
                pr: Box::new(pr),
                kind,
                why,
                result,
            });
        });
    }

    fn on_key(&mut self, key: KeyEvent, page: usize) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        match self.popup.take() {
            Some(Popup::Help) => return, // any key closes help
            Some(Popup::Pick(mut p)) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => self.submit_pick(*p),
                    KeyCode::Up | KeyCode::BackTab => {
                        p.choice = (p.choice + Kind::ALL.len() - 1) % Kind::ALL.len();
                        self.popup = Some(Popup::Pick(p));
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        p.choice = (p.choice + 1) % Kind::ALL.len();
                        self.popup = Some(Popup::Pick(p));
                    }
                    KeyCode::Backspace => {
                        p.why.pop();
                        self.popup = Some(Popup::Pick(p));
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if p.why.chars().count() < 200 {
                            p.why.push(ch);
                        }
                        self.popup = Some(Popup::Pick(p));
                    }
                    _ => self.popup = Some(Popup::Pick(p)),
                }
                return;
            }
            Some(Popup::Follows(ed)) => {
                self.follow_editor_key(ed, key);
                return;
            }
            None => {}
        }
        let page = page.max(1) as isize;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.popup = Some(Popup::Help),
            KeyCode::Char('/') => self.want_search = true,
            KeyCode::Char('v') if self.screen == Screen::Following => {
                self.toggle_following_grouping()
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.move_by(page),
            KeyCode::PageUp => self.move_by(-page),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_by(page / 2)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_by(-page / 2)
            }
            KeyCode::Home | KeyCode::Char('g') => self.cursor[self.screen.idx()] = 0,
            KeyCode::End | KeyCode::Char('G') => self.move_by(isize::MAX / 2),
            KeyCode::Tab => self.screen = Screen::ALL[(self.screen.idx() + 1) % 3],
            KeyCode::BackTab => self.screen = Screen::ALL[(self.screen.idx() + 2) % 3],
            KeyCode::Char('1') => self.screen = Screen::Board,
            KeyCode::Char('2') => self.screen = Screen::Mine,
            KeyCode::Char('3') => self.screen = Screen::Following,
            KeyCode::Char('p') => self.popup = Some(Popup::Follows(FollowEditor::default())),
            KeyCode::Char('w') => {
                self.only_followed = !self.only_followed;
                self.cursor[0] = 0;
                if self.only_followed && self.followed_authors.is_empty() {
                    self.say("you follow nobody yet: p to add people or teams", false);
                }
            }
            KeyCode::Char('+') => {
                if let Some(pr) = self.selected().cloned() {
                    self.follow(Who::User(pr.author.clone()), &pr.author);
                }
            }
            KeyCode::Char('f') => self.cycle_filter(false),
            KeyCode::Char('F') => self.cycle_filter(true),
            KeyCode::Enter | KeyCode::Char('o') => self.open_selected(),
            KeyCode::Char('c') | KeyCode::Char('b') => self.start_pick(),
            KeyCode::Char('x') => {
                if let Some(pr) = self.selected().cloned() {
                    self.mark_seen(&pr);
                    self.say(format!("marked {} as seen", pr.key()), false);
                }
            }
            KeyCode::Char('r') => {
                self.refresh();
                self.say("refreshing…", false);
            }
            _ => {}
        }
    }
}

impl App {
    fn follow_editor_key(&mut self, mut ed: FollowEditor, key: KeyEvent) {
        let entries = self.follows.all();
        if let Some(mut input) = ed.input.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    let org = self.cfg.orgs.first().cloned();
                    match Who::parse(&input, org.as_deref()) {
                        Ok(who) => match self.client.clone() {
                            Some(client) => {
                                ed.checking = true;
                                self.say(format!("checking {who}…"), false);
                                let tx = self.tx.clone();
                                std::thread::spawn(move || {
                                    let r = client.check(&who).map_err(|e| format!("{e:#}"));
                                    let _ = tx.send(Msg::FollowChecked(r));
                                });
                            }
                            None => self.say("no GitHub token", true),
                        },
                        Err(e) => {
                            self.say(format!("{e:#}"), true);
                            ed.input = Some(input);
                        }
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                    ed.input = Some(input);
                }
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if input.len() < 120 {
                        input.push(ch);
                    }
                    ed.input = Some(input);
                }
                _ => ed.input = Some(input),
            }
            self.popup = Some(Popup::Follows(ed));
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('p') => return,
            KeyCode::Down | KeyCode::Char('j') => {
                ed.cursor = (ed.cursor + 1).min(entries.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => ed.cursor = ed.cursor.saturating_sub(1),
            KeyCode::Char('a') | KeyCode::Char('+') | KeyCode::Char('i') => {
                ed.input = Some(String::new())
            }
            KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Char('-') | KeyCode::Delete => {
                if let Some(who) = entries.get(ed.cursor) {
                    self.unfollow(&who.clone());
                    ed.cursor = ed.cursor.min(entries.len().saturating_sub(2));
                }
            }
            _ => {}
        }
        self.popup = Some(Popup::Follows(ed));
    }
}

impl App {
    fn on_search(&mut self, outcome: std::io::Result<fzf::Outcome>) {
        match outcome {
            Ok(fzf::Outcome::Picked(pick)) => match self.jump_to(&pick.key) {
                Some(_) if pick.open => self.open_selected(),
                Some(pr) => self.say(format!("{} {}", pr.key(), pr.title), false),
                None => self.say(format!("{} is no longer listed", pick.key), true),
            },
            Ok(fzf::Outcome::Cancelled) => {}
            Ok(fzf::Outcome::Missing) => self.say(
                "search needs fzf: brew install fzf (or your package manager)",
                true,
            ),
            Err(e) => self.say(format!("fzf failed: {e}"), true),
        }
    }
}

fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

pub fn run(cfg: Config, follow_file: FollowFile) -> Result<()> {
    // Resolve the token before taking over the terminal, so a missing login
    // is a plain error message rather than a broken screen.
    let client = Arc::new(Client::from_env()?);
    let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
    let cache = Cache::new(config::cache_dir());
    let seen = Seen::load(config::state_dir());
    let mut app = App::new(cfg, Some(client), tx, cache, seen, follow_file);
    app.refresh();

    let mut terminal = ratatui::init();
    let result = (|| -> Result<()> {
        let mut page = 10;
        while !app.quit {
            terminal.draw(|f| page = view::draw(f, &mut app))?;
            if event::poll(Duration::from_millis(120))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        app.on_key(key, page);
                    }
                }
            }
            if std::mem::take(&mut app.want_search) {
                let lines = fzf::lines(&app);
                // Step out of the alternate screen and raw mode for fzf, then
                // step back in on the same Terminal (a fresh one would query
                // the cursor position, which not every pty answers).
                ratatui::restore();
                let outcome = fzf::run(&lines);
                ratatui::crossterm::terminal::enable_raw_mode()?;
                ratatui::crossterm::execute!(
                    std::io::stdout(),
                    ratatui::crossterm::terminal::EnterAlternateScreen
                )?;
                // resize() clears and forces a full redraw without the cursor
                // query that clear() makes
                let size = terminal.size()?;
                terminal.resize(ratatui::layout::Rect::new(0, 0, size.width, size.height))?;
                app.on_search(outcome);
            }
            while let Ok(msg) = rx.try_recv() {
                app.on_msg(msg);
            }
            app.tick = app.tick.wrapping_add(1);
            if let Some(every) = app.cfg.refresh() {
                if app.last_fetch.elapsed() >= every && !app.loading() {
                    app.refresh();
                }
            }
            if app
                .status
                .as_ref()
                .is_some_and(|s| !s.error && s.at.elapsed() > Duration::from_secs(6))
            {
                app.status = None;
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::parse_search_response;

    fn app_with(prs: Vec<Pr>) -> App {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(
            Config::default(),
            None,
            tx,
            Cache::new(None),
            Seen::in_memory(),
            FollowFile::new(None),
        );
        app.on_msg(Msg::Board(Ok(prs.clone())));
        app.on_msg(Msg::Mine(Ok(prs.clone())));
        let authors = prs.iter().map(|p| p.author.clone()).collect();
        app.on_msg(Msg::Following(Ok((prs, authors))));
        app
    }

    fn sample() -> Vec<Pr> {
        parse_search_response(include_str!("../../tests/fixtures/search.json")).unwrap()
    }

    #[test]
    fn board_sorts_by_kind_then_age() {
        let app = app_with(sample());
        let kinds: Vec<_> = app.items().iter().map(|p| p.kind()).collect();
        assert_eq!(
            kinds,
            vec![Some(Kind::RubberStamp), Some(Kind::Design), None]
        );
    }

    #[test]
    fn filter_cycles_and_narrows() {
        let mut app = app_with(sample());
        app.on_key(KeyEvent::from(KeyCode::Char('f')), 10);
        assert_eq!(app.filter, Some(Kind::RubberStamp));
        assert_eq!(app.items().len(), 1);
        app.on_key(KeyEvent::from(KeyCode::Char('F')), 10);
        assert_eq!(app.filter, None);
    }

    #[test]
    fn cursor_stays_in_bounds() {
        let mut app = app_with(sample());
        app.on_key(KeyEvent::from(KeyCode::Char('G')), 10);
        assert_eq!(app.cursor(), 2);
        app.on_key(KeyEvent::from(KeyCode::Char('j')), 10);
        assert_eq!(app.cursor(), 2);
        app.on_key(KeyEvent::from(KeyCode::Char('g')), 10);
        assert_eq!(app.cursor(), 0);
    }

    #[test]
    fn posting_updates_the_row_and_survives_a_stale_refresh() {
        let mut app = app_with(sample());
        let pr = app
            .board
            .iter()
            .find(|p| p.kind().is_none())
            .unwrap()
            .clone();
        app.on_msg(Msg::Posted {
            pr: Box::new(pr.clone()),
            kind: Kind::Ai,
            why: Some("boilerplate".into()),
            result: Ok(("https://x".into(), false)),
        });
        let find = |app: &App| {
            app.board
                .iter()
                .find(|p| p.key() == pr.key())
                .unwrap()
                .kind()
        };
        assert_eq!(find(&app), Some(Kind::Ai));
        // the search index has not seen the new comment yet
        app.on_msg(Msg::Board(Ok(sample())));
        assert_eq!(find(&app), Some(Kind::Ai));
    }

    #[test]
    fn picker_edits_reason_and_cycles_kind() {
        let mut app = app_with(sample());
        app.popup = Some(Popup::Pick(Box::new(Picker {
            pr: app.board[0].clone(),
            choice: 0,
            why: String::new(),
        })));
        for k in [
            KeyCode::Down,
            KeyCode::Char('h'),
            KeyCode::Char('i'),
            KeyCode::Backspace,
        ] {
            app.on_key(KeyEvent::from(k), 10);
        }
        match &app.popup {
            Some(Popup::Pick(p)) => {
                assert_eq!(p.choice, 1);
                assert_eq!(p.why, "h");
            }
            _ => panic!("picker closed"),
        }
        app.on_key(KeyEvent::from(KeyCode::Esc), 10);
        assert!(app.popup.is_none());
    }

    #[test]
    fn following_groups_by_author_or_repo() {
        let mut app = app_with(sample());
        app.screen = Screen::Following;
        let groups = |app: &App| -> Vec<String> {
            app.items()
                .iter()
                .map(|p| following_group(p, app.following_by_repo))
                .collect()
        };
        let mut sorted = groups(&app);
        sorted.sort();
        assert_eq!(groups(&app), sorted, "grouped by author");
        app.cursor[2] = 1;
        let before = app.selected().unwrap().key();
        app.on_key(KeyEvent::from(KeyCode::Char('v')), 10);
        assert!(app.following_by_repo);
        let mut sorted = groups(&app);
        sorted.sort();
        assert_eq!(groups(&app), sorted, "grouped by repo");
        assert_eq!(
            app.selected().unwrap().key(),
            before,
            "cursor follows the PR"
        );
    }

    #[test]
    fn search_pick_jumps_across_screens_and_clears_hiding_filters() {
        let mut app = app_with(sample());
        app.board.retain(|p| p.number != 7);
        app.filter = Some(Kind::Design);
        // #7 is on My PRs (and Following), not the board
        app.on_search(Ok(fzf::Outcome::Picked(fzf::Pick {
            key: "Autofleet/web#7".into(),
            open: false,
        })));
        assert_ne!(app.screen, Screen::Board);
        assert_eq!(app.filter, None, "the kind filter would hide it");
        assert_eq!(app.selected().unwrap().number, 7);
        app.on_search(Ok(fzf::Outcome::Missing));
        assert!(app.status.as_ref().unwrap().error);
    }

    #[test]
    fn slash_requests_search() {
        let mut app = app_with(sample());
        app.on_key(KeyEvent::from(KeyCode::Char('/')), 10);
        assert!(app.want_search);
    }

    #[test]
    fn tab_cycles_three_screens() {
        let mut app = app_with(sample());
        for want in [Screen::Mine, Screen::Following, Screen::Board] {
            app.on_key(KeyEvent::from(KeyCode::Tab), 10);
            assert_eq!(app.screen, want);
        }
        app.on_key(KeyEvent::from(KeyCode::BackTab), 10);
        assert_eq!(app.screen, Screen::Following);
    }

    #[test]
    fn following_groups_by_author_and_board_can_filter_to_followed() {
        let mut app = app_with(sample());
        app.screen = Screen::Following;
        let authors: Vec<_> = app.items().iter().map(|p| p.author.clone()).collect();
        let mut sorted = authors.clone();
        sorted.sort_by_key(|a| a.to_lowercase());
        assert_eq!(authors, sorted);

        app.screen = Screen::Board;
        app.followed_authors = ["alice".to_string()].into_iter().collect();
        app.on_key(KeyEvent::from(KeyCode::Char('w')), 10);
        assert!(app.only_followed);
        assert!(app.items().iter().all(|p| p.author == "alice"));
        assert_eq!(app.items().len(), 1);
    }

    #[test]
    fn follow_editor_adds_and_removes() {
        let mut app = app_with(sample());
        // `+` follows the selected PR's author without asking GitHub
        app.on_key(KeyEvent::from(KeyCode::Char('+')), 10);
        let first = app.board[0].author.clone();
        assert_eq!(app.follows.users, vec![first.clone()]);
        // the editor removes it again
        app.on_key(KeyEvent::from(KeyCode::Char('p')), 10);
        assert!(matches!(app.popup, Some(Popup::Follows(_))));
        app.on_key(KeyEvent::from(KeyCode::Char('d')), 10);
        assert!(app.follows.users.is_empty());
        // typing a bad name keeps the input open with an error
        app.on_key(KeyEvent::from(KeyCode::Char('a')), 10);
        for ch in "a b".chars() {
            app.on_key(KeyEvent::from(KeyCode::Char(ch)), 10);
        }
        app.on_key(KeyEvent::from(KeyCode::Enter), 10);
        match &app.popup {
            Some(Popup::Follows(ed)) => assert_eq!(ed.input.as_deref(), Some("a b")),
            _ => panic!("editor closed"),
        }
        assert!(app.status.as_ref().unwrap().error);
        app.on_key(KeyEvent::from(KeyCode::Esc), 10);
        app.on_key(KeyEvent::from(KeyCode::Esc), 10);
        assert!(app.popup.is_none());
    }

    #[test]
    fn renders_without_panicking_at_many_sizes() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let mut app = app_with(sample());
        for (w, h) in [(20, 5), (60, 15), (100, 30), (160, 50)] {
            for screen in Screen::ALL {
                app.screen = screen;
                for popup in [0, 1, 2, 3, 4] {
                    app.popup = match popup {
                        1 => Some(Popup::Help),
                        3 => Some(Popup::Follows(FollowEditor::default())),
                        4 => Some(Popup::Follows(FollowEditor {
                            cursor: 0,
                            input: Some("someone".into()),
                            checking: true,
                        })),
                        2 => Some(Popup::Pick(Box::new(Picker {
                            pr: app.board[0].clone(),
                            choice: 2,
                            why: "why not".into(),
                        }))),
                        _ => None,
                    };
                    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
                    t.draw(|f| {
                        view::draw(f, &mut app);
                    })
                    .unwrap();
                }
            }
        }
    }
}

/// `cargo test print_screens -- --ignored --nocapture` prints the screens as
/// plain text; that is where the README's text screenshots come from.
#[cfg(test)]
mod screens {
    use super::*;
    use crate::github::parse_search_response;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    #[ignore]
    fn print_screens() {
        let (tx, _rx) = mpsc::channel();
        let mut app = App::new(
            Config::default(),
            None,
            tx,
            Cache::new(None),
            Seen::in_memory(),
            FollowFile::new(None),
        );
        let prs = parse_search_response(include_str!("../../tests/fixtures/demo.json")).unwrap();
        app.on_msg(Msg::Board(Ok(prs
            .iter()
            .filter(|p| p.request.is_some())
            .cloned()
            .collect())));
        // pretend you last looked at infra#88 before its newest review comment
        if let Some(pr) = prs.iter().find(|p| p.number == 88) {
            app.seen.mark(pr, pr.created_at);
        }
        app.on_msg(Msg::Mine(Ok(prs
            .iter()
            .filter(|p| p.activity.is_some())
            .cloned()
            .collect())));
        let w: u16 = std::env::var("W")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);
        let h: u16 = std::env::var("H")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(24);
        let followed = ["dana", "lior", "maya", "noa"];
        for u in followed {
            app.follows.add(&Who::User(u.into()));
        }
        app.follows.add(&Who::Team {
            org: "acme".into(),
            slug: "dev-ordering".into(),
        });
        app.on_msg(Msg::Following(Ok((
            prs.iter()
                .filter(|p| followed.contains(&p.author.as_str()))
                .cloned()
                .collect(),
            followed.iter().map(|s| s.to_string()).collect(),
        ))));
        let popup = std::env::var("POPUP").is_ok();
        for screen in Screen::ALL {
            app.screen = screen;
            if popup && screen == Screen::Following {
                app.popup = Some(Popup::Follows(FollowEditor {
                    cursor: 1,
                    input: Some("galinareznikk".into()),
                    checking: false,
                }));
            }
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| {
                view::draw(f, &mut app);
            })
            .unwrap();
            let buf = t.backend().buffer();
            for y in 0..h {
                // solid pixels are coloured blank cells; show them as █ in text
                let line: String = (0..w)
                    .map(|x| {
                        let c = &buf[(x, y)];
                        let pixel = c.symbol() == " "
                            && matches!(c.bg, ratatui::style::Color::Rgb(..))
                            && c.fg == ratatui::style::Color::Reset;
                        if pixel {
                            "█".to_string()
                        } else {
                            c.symbol().to_string()
                        }
                    })
                    .collect();
                println!("{}", line.trim_end());
            }
            println!();
        }
    }
}
