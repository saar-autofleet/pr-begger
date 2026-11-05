//! `/` search: hand the PRs to fzf, then jump to (or open) the pick.
//!
//! fzf draws on `/dev/tty` itself, so the TUI steps aside while it runs and
//! takes the terminal back afterwards. Every PR from all three screens is
//! offered once, labelled with where it lives, so you can find a PR without
//! first switching to the right screen.

use std::io::Write;
use std::process::{Command, Stdio};

use super::{App, Screen};
use crate::model::Pr;

/// What to do with the picked PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub key: String,
    pub open: bool,
}

fn screen_name(s: Screen) -> &'static str {
    match s {
        Screen::Board => "begging",
        Screen::Mine => "mine",
        Screen::Following => "following",
    }
}

/// One tab-separated line per PR: a hidden key, then the shown fields.
/// The number appears as both `#4202` and `4202`'s digits, so typing either
/// finds it; titles, repo, author and kind are all searchable.
pub fn lines(app: &App) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    // the current screen first, so its PRs rank first on ties
    let mut order = vec![app.screen];
    order.extend(Screen::ALL.iter().copied().filter(|s| *s != app.screen));
    let mut out = Vec::new();
    for screen in order {
        for pr in app.list(screen) {
            if seen.insert(pr.key()) {
                out.push(line(pr, screen));
            }
        }
    }
    out
}

fn line(pr: &Pr, screen: Screen) -> String {
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    format!(
        "{}\t{:<6}\t{}#{}\t{}\t@{}\t[{}]",
        pr.key(),
        pr.kind().map_or("–", |k| match k {
            crate::kind::Kind::RubberStamp => "stamp",
            crate::kind::Kind::Ai => "ai",
            crate::kind::Kind::Design => "design",
        }),
        pr.short_repo(),
        pr.number,
        clean(&pr.title),
        pr.author,
        screen_name(screen),
    )
}

/// fzf's output with `--expect`: the key pressed, then the picked line.
pub fn parse(output: &str) -> Option<Pick> {
    let mut it = output.lines();
    let pressed = it.next()?;
    let picked = it.next()?;
    let key = picked.split('\t').next()?.trim();
    (!key.is_empty()).then(|| Pick {
        key: key.to_string(),
        open: pressed == "ctrl-o",
    })
}

pub enum Outcome {
    Picked(Pick),
    Cancelled,
    Missing,
}

/// Exact matching: every typed word must appear as a substring (in any
/// order). fzf's default fuzzy mode matched letters scattered across a
/// line, so `nested` hit "add readi*nes*sPa*t*h …". A word starting with `'`
/// is still matched fuzzily, `^`/`$` anchor, `!` excludes.
const ARGS: &[&str] = &[
    "--exact",
    "--delimiter=\t",
    "--with-nth=2..",
    "--layout=reverse",
    "--border=rounded",
    "--border-label= PR Begger · search ",
    "--prompt=beg/ ",
    "--pointer=▸",
    "--header=words match exactly, any order · 'word fuzzy · !word exclude\nenter jump to it · ctrl-o open in browser · esc back",
    "--expect=ctrl-o",
    "--color=hl:#9480b8,hl+:#c8b6e2,pointer:#e292a8,prompt:#a7c7e7,border:#a096b4,label:#c8b6e2,header:#a096b4,info:#a096b4,marker:#aadebe",
];

/// Run fzf over `lines`. The caller must have released the terminal.
pub fn run(lines: &[String]) -> std::io::Result<Outcome> {
    let child = Command::new("fzf")
        .args(ARGS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Outcome::Missing),
        Err(e) => return Err(e),
    };
    if let Some(mut stdin) = child.stdin.take() {
        // fzf may exit before reading everything (esc); ignore broken pipes
        let _ = stdin.write_all(lines.join("\n").as_bytes());
    }
    let out = child.wait_with_output()?;
    // 0 picked, 1 no match, 130 esc / ctrl-c
    Ok(match parse(&String::from_utf8_lossy(&out.stdout)) {
        Some(p) if out.status.success() => Outcome::Picked(p),
        _ => Outcome::Cancelled,
    })
}

impl App {
    /// Move to the picked PR: stay on this screen if it is here, else switch
    /// to the screen that has it, clearing filters that would hide it.
    pub fn jump_to(&mut self, key: &str) -> Option<Pr> {
        let mut order = vec![self.screen];
        order.extend(Screen::ALL.iter().copied().filter(|s| *s != self.screen));
        let screen = order
            .into_iter()
            .find(|s| self.list(*s).iter().any(|p| p.key() == key))?;
        self.screen = screen;
        if !self.items().iter().any(|p| p.key() == key) {
            self.filter = None;
            self.only_followed = false;
        }
        let idx = self.items().iter().position(|p| p.key() == key)?;
        self.cursor[screen.idx()] = idx;
        self.items().get(idx).map(|p| (*p).clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exactly_not_fuzzily() {
        assert!(ARGS.contains(&"--exact"));
        assert!(!ARGS.iter().any(|a| a.starts_with("--tiebreak=index")));
    }

    #[test]
    fn parses_enter_and_ctrl_o() {
        let line = "Autofleet/api#812\tstamp \tapi#812\tBump serde\t@dana\t[begging]";
        assert_eq!(
            parse(&format!("\n{line}\n")),
            Some(Pick {
                key: "Autofleet/api#812".into(),
                open: false
            })
        );
        assert!(parse(&format!("ctrl-o\n{line}\n")).unwrap().open);
        assert_eq!(parse(""), None);
        assert_eq!(parse("\n"), None);
    }

    #[test]
    fn lines_hide_the_key_and_strip_tabs() {
        let pr =
            crate::github::parse_search_response(include_str!("../../tests/fixtures/search.json"))
                .unwrap()
                .remove(0);
        let mut pr = pr;
        pr.title = "tab\there\nnewline".into();
        let l = line(&pr, Screen::Mine);
        let fields: Vec<&str> = l.split('\t').collect();
        assert_eq!(fields.len(), 6);
        assert_eq!(fields[0], "Autofleet/api#101");
        assert_eq!(fields[2], "api#101");
        assert_eq!(fields[3], "tab here newline");
        assert_eq!(fields[5], "[mine]");
    }
}
