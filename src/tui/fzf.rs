//! `/` search: hand the PRs to fzf, then jump to (or open) the pick.
//!
//! fzf draws on `/dev/tty` itself, so the TUI steps aside while it runs and
//! takes the terminal back afterwards. Every PR from all three screens is
//! offered once, labelled with where it lives, so you can find a PR without
//! first switching to the right screen.

use std::io::Write;
use std::process::{Command, Stdio};

use unicode_width::UnicodeWidthStr;

use super::pixel::kind_color;
use super::view::fit;
use super::{App, Screen};
use crate::model::{age, Pr};

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

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

fn fg(c: ratatui::style::Color) -> String {
    match c {
        ratatui::style::Color::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        _ => String::new(),
    }
}

/// One line per PR, three tab-separated fields: a hidden key, the aligned
/// row fzf shows and searches (kind dot, `repo#n`, title, author, kind), and
/// a hidden detail block for the preview pane. `width` is the terminal's.
pub fn lines(app: &App, width: u16) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    // the current screen first, so its PRs rank first on ties
    let mut order = vec![app.screen];
    order.extend(Screen::ALL.iter().copied().filter(|s| *s != app.screen));
    let mut prs: Vec<(&Pr, Screen)> = Vec::new();
    for screen in order {
        for pr in app.list(screen) {
            if seen.insert(pr.key()) {
                prs.push((pr, screen));
            }
        }
    }
    let repo_w = prs
        .iter()
        .map(|(p, _)| format!("{}#{}", p.short_repo(), p.number).width())
        .max()
        .unwrap_or(8)
        .min(24);
    let author_w = prs
        .iter()
        .map(|(p, _)| p.author.width())
        .max()
        .unwrap_or(8)
        .min(20);
    // border 2 + pointer 2 + dot 2 + repo + gap + gap + author + gap + kind 13
    let title_w = (width as usize)
        .saturating_sub(repo_w + author_w + 24)
        .clamp(24, 110);
    prs.into_iter()
        .map(|(pr, screen)| line(pr, screen, repo_w, title_w, author_w))
        .collect()
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn line(pr: &Pr, screen: Screen, repo_w: usize, title_w: usize, author_w: usize) -> String {
    let dot = match pr.kind() {
        Some(k) => format!("{}●{RESET}", fg(kind_color(k))),
        None => format!("{DIM}·{RESET}"),
    };
    let kind = pr
        .kind()
        .map_or(String::new(), |k| k.label().to_lowercase());
    let shown = format!(
        "{dot} {DIM}{}{RESET} {} {DIM}{} {kind}{RESET}",
        fit(&format!("{}#{}", pr.short_repo(), pr.number), repo_w),
        fit(&clean(&pr.title), title_w),
        fit(&pr.author, author_w),
    );
    format!("{}\t{shown}\t{}", pr.key(), detail(pr, screen))
}

/// The preview pane's text, as one line with `\n` escapes for `printf %b`.
fn detail(pr: &Pr, screen: Screen) -> String {
    let now = chrono::Utc::now();
    let mut out = vec![
        format!("{BOLD}{}{RESET}", clean(&pr.title)),
        format!(
            "{DIM}{} · {} · opened {} ago · {} · {}{RESET}",
            pr.key(),
            pr.author,
            age(pr.created_at, now),
            screen_name(screen),
            pr.size_long()
        ),
    ];
    out.push(match &pr.request {
        Some(r) => format!(
            "{}● {}{RESET}{}",
            fg(kind_color(r.kind)),
            r.kind.label(),
            r.why
                .as_deref()
                .map_or(String::new(), |w| format!(" — {}", clean(w)))
        ),
        None => format!("{DIM}no review kind requested{RESET}"),
    });
    out.push(format!(
        "ci {} · review {} · merge {}",
        pr.ci.word(),
        pr.review.word(),
        pr.merge.word()
    ));
    // printf %b would read a backslash in a title as an escape
    out.iter()
        .map(|l| l.replace('\\', "\\\\"))
        .collect::<Vec<_>>()
        .join("\\n")
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
    "--ansi",
    "--delimiter=\t",
    "--with-nth=2",
    "--layout=reverse",
    "--border=rounded",
    "--border-label= PR Begger · search ",
    "--prompt=beg/ ",
    "--pointer=▸",
    "--info=inline-right",
    "--no-separator",
    "--header=enter jump · ctrl-o open in browser · esc back   ('word fuzzy, !word exclude)",
    "--expect=ctrl-o",
    "--preview=printf '%b' {3}",
    "--preview-window=down,5,wrap,border-top",
    "--color=hl:#9480b8,hl+:#9480b8,pointer:#e292a8,prompt:#a7c7e7,border:#a096b4,label:#c8b6e2,header:#a096b4,info:#a096b4,marker:#aadebe,preview-border:#a096b4",
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
    fn lines_have_key_row_and_detail() {
        let mut pr =
            crate::github::parse_search_response(include_str!("../../tests/fixtures/search.json"))
                .unwrap()
                .remove(0);
        pr.title = "tab\there\nnewline \\ slash".into();
        let l = line(&pr, Screen::Mine, 10, 30, 8);
        let fields: Vec<&str> = l.split('\t').collect();
        assert_eq!(fields.len(), 3, "{l:?}");
        assert_eq!(fields[0], "Autofleet/api#101");
        assert!(fields[1].contains("api#101"));
        assert!(fields[1].contains("tab here newline"));
        assert!(fields[1].contains("design review"), "kind is searchable");
        assert!(!fields[2].contains('\n'), "preview text stays on one line");
        assert!(fields[2].contains("\\n"), "with \\n escapes for printf");
        assert!(fields[2].contains("\\\\ slash"), "backslashes escaped");
        assert!(fields[2].contains("new queue topology"));
    }

    #[test]
    fn rows_align() {
        let prs =
            crate::github::parse_search_response(include_str!("../../tests/fixtures/search.json"))
                .unwrap();
        let strip = |s: &str| {
            let mut out = String::new();
            let mut esc = false;
            for ch in s.chars() {
                match (esc, ch) {
                    (false, '\x1b') => esc = true,
                    (true, 'm') => esc = false,
                    (false, c) => out.push(c),
                    _ => {}
                }
            }
            out
        };
        let widths: Vec<usize> = prs
            .iter()
            .map(|p| {
                let l = line(p, Screen::Board, 12, 30, 8);
                strip(l.split('\t').nth(1).unwrap()).width()
                    - p.kind().map_or(0, |k| k.label().len())
            })
            .collect();
        assert!(widths.windows(2).all(|w| w[0] == w[1]), "{widths:?}");
    }
}

/// `cargo test print_search_rows -- --ignored --nocapture` prints the rows
/// fzf would show for the cached board (ANSI stripped), to eyeball layout.
#[cfg(test)]
mod preview {
    use super::*;
    use crate::config::Config;
    use crate::follow::FollowFile;
    use crate::store::{Cache, Seen};

    #[test]
    #[ignore]
    fn print_search_rows() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let app = App::new(
            Config::default(),
            None,
            tx,
            Cache::new(crate::config::cache_dir()),
            Seen::in_memory(),
            FollowFile::new(None),
        );
        let width: u16 = std::env::var("W")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(160);
        for l in lines(&app, width).iter().take(12) {
            let row = l.split('\t').nth(1).unwrap_or("");
            let mut out = String::new();
            let mut esc = false;
            for ch in row.chars() {
                match (esc, ch) {
                    (false, '\x1b') => esc = true,
                    (true, 'm') => esc = false,
                    (false, c) => out.push(c),
                    _ => {}
                }
            }
            println!("ROW {out}");
        }
    }
}
