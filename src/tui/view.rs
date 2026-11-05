//! Rendering. Everything here is a pure function of `App` plus the frame.

use chrono::Utc;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::pixel::{self, kind_color, pal};
use super::{App, FollowEditor, Picker, Popup, Screen};
use crate::follow::Who;
use crate::kind::Kind;
use crate::model::{age, Ci, Decision, Merge, Pr};

const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];
const DETAIL_MIN_WIDTH: u16 = 112;
const DETAIL_WIDTH: u16 = 40;

fn muted() -> Style {
    Style::new().add_modifier(Modifier::DIM)
}

/// Truncate to `w` display cells (with `…`) and pad to exactly `w`.
pub fn fit(s: &str, w: usize) -> String {
    if w == 0 {
        return String::new();
    }
    if s.width() <= w {
        return format!("{s}{}", " ".repeat(w - s.width()));
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if used + cw > w - 1 {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    used += 1;
    out.push_str(&" ".repeat(w.saturating_sub(used)));
    out
}

fn rfit(s: &str, w: usize) -> String {
    if s.width() >= w {
        fit(s, w)
    } else {
        format!("{}{s}", " ".repeat(w - s.width()))
    }
}

/// Small kind cue for a row: a pastel dot and the kind's short name.
/// Always 9 cells.
fn marker(kind: Option<Kind>) -> Vec<Span<'static>> {
    match kind {
        Some(k) => vec![
            Span::styled("● ", Style::new().fg(kind_color(k))),
            Span::styled(fit(short(k), 7), muted()),
        ],
        None => vec![Span::styled(fit("○ ?", 9), muted())],
    }
}

fn short(k: Kind) -> &'static str {
    match k {
        Kind::RubberStamp => "stamp",
        Kind::Ai => "ai",
        Kind::Design => "design",
    }
}

/// A Win95-style title bar: pastel bar, ink text, a close box on the right.
fn title_bar(text: &str, width: u16, color: Color) -> Line<'static> {
    let w = width.saturating_sub(2) as usize;
    let right = " × ";
    let left = fit(&format!(" {text}"), w.saturating_sub(right.width()));
    Line::from(vec![
        Span::styled(left, Style::new().fg(pal::INK).bg(color).bold()),
        Span::styled(right, Style::new().fg(pal::INK).bg(color)),
    ])
}

fn window(title: &str, width: u16, color: Color) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(muted())
        .title(title_bar(title, width, color))
}

fn ci_span(ci: Ci) -> Span<'static> {
    match ci {
        Ci::Passing => Span::styled("✓", Style::new().fg(Color::Green)),
        Ci::Failing => Span::styled("✗", Style::new().fg(Color::Red)),
        Ci::Running => Span::styled("◐", Style::new().fg(Color::Yellow)),
        Ci::None => Span::styled("·", muted()),
    }
}

fn review_span(pr: &Pr) -> Span<'static> {
    match pr.review.decision {
        Decision::Approved => Span::styled("✓", Style::new().fg(Color::Green)),
        Decision::ChangesRequested => Span::styled("✗", Style::new().fg(Color::Red)),
        Decision::Required => Span::styled("○", Style::new().fg(Color::Yellow)),
        Decision::None if pr.review.approvals > 0 => {
            Span::styled("✓", Style::new().fg(Color::Green))
        }
        Decision::None if pr.review.pending > 0 => {
            Span::styled("○", Style::new().fg(Color::Yellow))
        }
        Decision::None => Span::styled("·", muted()),
    }
}

fn merge_span(m: Merge, w: usize) -> Span<'static> {
    let (word, style) = match m {
        Merge::Ready => ("ready", Style::new().fg(Color::Green)),
        Merge::Conflict => ("conflict", Style::new().fg(Color::Red)),
        Merge::Behind => ("behind", muted()),
        Merge::Blocked => ("blocked", muted()),
        Merge::Unstable => ("unstable", muted()),
        Merge::Draft => ("draft", muted()),
        Merge::Unknown => ("…", muted()),
    };
    Span::styled(fit(word, w), style)
}

/// Draw the whole screen; returns the list's visible height (for paging).
pub fn draw(f: &mut Frame, app: &mut App) -> usize {
    let area = f.area();
    if area.width < 24 || area.height < 6 {
        f.render_widget(Paragraph::new("PR Begger: make me bigger"), area);
        return 1;
    }
    let tall = area.height >= 18 && area.width >= 52;
    let [head, body, foot] = Layout::vertical([
        Constraint::Length(if tall { 4 } else { 1 }),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    if tall {
        draw_header(f, app, head);
    } else {
        draw_compact_header(f, app, head);
    }

    let (list_area, detail_area) = if body.width >= DETAIL_MIN_WIDTH {
        let [l, d] =
            Layout::horizontal([Constraint::Min(40), Constraint::Length(DETAIL_WIDTH)]).areas(body);
        (l, Some(d))
    } else {
        (body, None)
    };
    let (page, scroll) = draw_list(f, app, list_area);
    app.scroll[app.screen.idx()] = scroll;
    if let Some(d) = detail_area {
        draw_detail(f, app, d);
    }
    draw_footer(f, app, foot);

    match &app.popup {
        Some(Popup::Help) => draw_help(f, area),
        Some(Popup::Pick(p)) => draw_picker(f, p, area),
        Some(Popup::Follows(ed)) => draw_follows(f, app, ed, area),
        None => {}
    }
    page
}

fn tabs(app: &App) -> Line<'static> {
    let new = app.new_keys.len();
    let tab = |label: String, on: bool| {
        if on {
            Span::styled(label, Style::new().fg(pal::INK).bg(pal::LILAC).bold())
        } else {
            Span::styled(label, muted())
        }
    };
    let mut spans = vec![
        tab(
            format!(" Begging {} ", app.board.len()),
            app.screen == Screen::Board,
        ),
        Span::raw(" "),
        tab(
            format!(" My PRs {} ", app.mine.len()),
            app.screen == Screen::Mine,
        ),
        Span::raw(" "),
        tab(
            format!(" Following {} ", app.following.len()),
            app.screen == Screen::Following,
        ),
    ];
    if new > 0 {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!(" {new} new "),
            Style::new().fg(pal::INK).bg(pal::PINK),
        ));
    }
    if app.only_followed && app.screen == Screen::Board {
        spans.push(Span::styled("  followed only", muted()));
    }
    if let Some(k) = app.filter {
        spans.push(Span::styled("  only ", muted()));
        spans.extend(marker(Some(k)));
    }
    Line::from(spans)
}

/// Right side of the status bar: fetch state, freshness, scope.
fn status_line(app: &App) -> Line<'static> {
    let now = Utc::now();
    let at = match app.screen {
        Screen::Board => app.board_at,
        Screen::Mine => app.mine_at,
        Screen::Following => app.following_at,
    };
    let mut spans = Vec::new();
    if app.loading() {
        spans.push(Span::styled(
            format!(
                "{} fetching · ",
                SPINNER[(app.tick / 3) as usize % SPINNER.len()]
            ),
            muted(),
        ));
    }
    spans.push(Span::styled(
        match at {
            Some(t) if age(t, now) == "now" => "updated just now · ".into(),
            Some(t) => format!("updated {} ago · ", age(t, now)),
            None => "not fetched yet · ".into(),
        },
        muted(),
    ));
    spans.push(Span::styled(format!("{} ", app.scope_label()), muted()));
    Line::from(spans)
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let banner = pixel::banner("PR BEGGER");
    let (bw, _) = banner.cells();
    let left_w = (bw + 2).max(36).min(area.width);
    let [left, right] =
        Layout::horizontal([Constraint::Length(left_w), Constraint::Min(0)]).areas(area);
    banner.render(left.x + 1, left.y, left, f.buffer_mut());
    let tabs_row = Rect::new(area.x, area.y + 3, area.width, 1);
    f.render_widget(Paragraph::new(tabs(app)), tabs_row);
    draw_legend(f, app, right);
}

fn draw_compact_header(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled(" PR Begger ", Style::new().fg(pal::INK).bg(pal::SKY).bold()),
        Span::raw(" "),
    ];
    spans.extend(tabs(app).spans);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A small icon per kind with its count, like a 90s desktop shortcut row.
/// With a filter on, the other kinds fade.
fn draw_legend(f: &mut Frame, app: &App, area: Rect) {
    const ENTRY_W: u16 = 22;
    if area.width < ENTRY_W * 3 || area.height < 4 {
        return;
    }
    let list = app.list(app.screen);
    // icons are 4 rows: they may use the tab row, which is empty this far right
    // right-aligned, so a long tab row (new count, filter) does not run into it
    let w = ENTRY_W * 3;
    let area = Rect::new(area.right() - w, area.y, w, area.height.min(4));
    for (i, k) in Kind::ALL.into_iter().enumerate() {
        let x = area.x + i as u16 * ENTRY_W;
        let r = Rect::new(x, area.y, ENTRY_W.min(area.right() - x), area.height);
        let faded = app.filter.is_some_and(|fk| fk != k);
        if !faded {
            pixel::mini(k).render(r.x, r.y, r, f.buffer_mut());
        }
        let count = list.iter().filter(|p| p.kind() == Some(k)).count();
        let name = match k {
            Kind::RubberStamp => "Rubber stamp",
            Kind::Ai => "AI review",
            Kind::Design => "Design",
        };
        let style = if faded { muted() } else { Style::new() };
        let text = vec![
            Line::from(Span::styled(name, style)),
            Line::from(vec![
                Span::styled(format!("{count}"), style.bold()),
                Span::styled(if count == 1 { " PR" } else { " PRs" }, muted()),
            ]),
            Line::from(Span::styled(
                match k {
                    Kind::RubberStamp => "just approve",
                    Kind::Ai => "bot reviews",
                    Kind::Design => "humans think",
                },
                muted(),
            )),
        ];
        let tr = Rect::new(r.x + 9, r.y, r.width.saturating_sub(9), 3);
        f.render_widget(Paragraph::new(text), tr);
    }
}

enum Row<'a> {
    Header(Line<'a>),
    Item(usize, &'a Pr),
}

struct Cols {
    repo: usize,
    author: usize,
    size: usize,
    title: usize,
}

fn columns(app: &App, items: &[&Pr], width: usize) -> Cols {
    let repo = items
        .iter()
        .map(|p| format!("{}#{}", p.short_repo(), p.number).width())
        .max()
        .unwrap_or(6)
        .clamp(6, 24);
    // mark + marker + repo + gap + age + ci + review
    let mut fixed = 1 + 9 + repo + 1 + 1 + 4 + 1 + 1 + 1 + 1;
    let mut author = 0;
    let mut size = 0;
    match app.screen {
        Screen::Following => {
            // grouped by author, so no author column
            if width.saturating_sub(fixed + 11) >= 24 {
                size = 11;
                fixed += 11;
            }
        }
        Screen::Board => {
            for (col, w) in [(&mut size, 11usize), (&mut author, 12usize)] {
                if width.saturating_sub(fixed + w) >= 24 {
                    *col = w;
                    fixed += w;
                }
            }
        }
        Screen::Mine => {
            // new flag + threads + merge
            fixed += 2 + 6 + 9;
            if width.saturating_sub(fixed + 11) >= 24 {
                size = 11;
                fixed += 11;
            }
        }
    }
    Cols {
        repo,
        author,
        size,
        title: width.saturating_sub(fixed),
    }
}

/// A thin rule with the group's name: `── Rubber stamp 3 ───── blurb`.
fn group_header(kind: Option<Kind>, count: usize, width: usize) -> Line<'static> {
    let (label, blurb, color) = match kind {
        Some(k) => (k.label(), k.blurb(), kind_color(k)),
        None => ("No kind yet", "no readable request", pal::FADE),
    };
    rule_line(label, count, blurb, color, width)
}

fn rule_line(label: &str, count: usize, note: &str, color: Color, width: usize) -> Line<'static> {
    let head = format!(" {label} ");
    let num = format!("{count} ");
    let note = if note.is_empty() {
        String::new()
    } else {
        format!(" {note}")
    };
    let used = 3 + head.width() + num.width() + note.width();
    let fill = width.saturating_sub(used).min(6);
    Line::from(vec![
        Span::styled("───", Style::new().fg(color)),
        Span::styled(head, Style::new().bold()),
        Span::styled(num, muted()),
        Span::styled("─".repeat(fill), Style::new().fg(color)),
        Span::styled(note, muted()),
    ])
}

fn mine_header(new: bool, count: usize, width: usize) -> Line<'static> {
    if new {
        rule_line(
            "New activity",
            count,
            "since you last looked · x marks seen",
            pal::PINK,
            width,
        )
    } else {
        rule_line("My open PRs", count, "", pal::FADE, width)
    }
}

fn rows<'a>(app: &'a App, items: &[&'a Pr], width: usize) -> Vec<Row<'a>> {
    let mut out = Vec::new();
    match app.screen {
        Screen::Board => {
            let mut last: Option<Option<Kind>> = None;
            for (i, pr) in items.iter().enumerate() {
                if last != Some(pr.kind()) {
                    let n = items.iter().filter(|p| p.kind() == pr.kind()).count();
                    out.push(Row::Header(group_header(pr.kind(), n, width)));
                    last = Some(pr.kind());
                }
                out.push(Row::Item(i, pr));
            }
        }
        Screen::Following => {
            let mut last: Option<String> = None;
            for (i, pr) in items.iter().enumerate() {
                let who = pr.author.to_lowercase();
                if last.as_deref() != Some(who.as_str()) {
                    let n = items
                        .iter()
                        .filter(|p| p.author.to_lowercase() == who)
                        .count();
                    out.push(Row::Header(rule_line(&pr.author, n, "", pal::LILAC, width)));
                    last = Some(who);
                }
                out.push(Row::Item(i, pr));
            }
        }
        Screen::Mine => {
            let mut last: Option<bool> = None;
            for (i, pr) in items.iter().enumerate() {
                let new = app.new_keys.contains(&pr.key());
                if last != Some(new) {
                    let n = items
                        .iter()
                        .filter(|p| app.new_keys.contains(&p.key()) == new)
                        .count();
                    out.push(Row::Header(mine_header(new, n, width)));
                    last = Some(new);
                }
                out.push(Row::Item(i, pr));
            }
        }
    }
    out
}

fn item_line(app: &App, pr: &Pr, cols: &Cols, selected: bool) -> Line<'static> {
    let now = Utc::now();
    let mut spans = vec![Span::raw(if selected { "▸" } else { " " })];
    spans.extend(marker(pr.kind()));
    spans.push(Span::styled(
        fit(&format!("{}#{}", pr.short_repo(), pr.number), cols.repo),
        muted(),
    ));
    spans.push(Span::raw(" "));
    let title_style = if pr.draft { muted() } else { Style::new() };
    if app.screen == Screen::Mine {
        let new = app.new_keys.contains(&pr.key());
        spans.push(Span::styled(
            if new { "● " } else { "  " },
            Style::new().fg(pal::ROSE),
        ));
    }
    spans.push(Span::styled(fit(&pr.title, cols.title), title_style));
    if cols.author > 0 {
        spans.push(Span::styled(
            fit(&format!(" {}", pr.author), cols.author),
            muted(),
        ));
    }
    if cols.size > 0 {
        spans.push(Span::styled(
            rfit(&format!("+{} −{}", pr.additions, pr.deletions), cols.size),
            muted(),
        ));
    }
    if app.screen == Screen::Mine {
        let a = pr.activity.clone().unwrap_or_default();
        let threads = if a.total_threads == 0 {
            Span::styled(rfit("–", 6), muted())
        } else if a.unresolved_threads > 0 {
            Span::raw(rfit(
                &format!("◆{}/{}", a.unresolved_threads, a.total_threads),
                6,
            ))
        } else {
            Span::styled(rfit(&format!("◇0/{}", a.total_threads), 6), muted())
        };
        spans.push(threads);
        spans.push(Span::raw(" "));
        spans.push(merge_span(pr.merge, 8));
    }
    spans.push(Span::styled(rfit(&age(pr.created_at, now), 4), muted()));
    spans.push(Span::raw(" "));
    spans.push(ci_span(pr.ci));
    spans.push(Span::raw(" "));
    spans.push(review_span(pr));
    let mut line = Line::from(spans);
    if selected {
        // the classic highlight bar, in lilac instead of navy
        line = line.style(Style::new().fg(pal::INK).bg(pal::LILAC));
    }
    line
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) -> (usize, usize) {
    let items = app.items();
    let height = area.height as usize;
    if items.is_empty() {
        let msg: Vec<Line> = if app.loading() {
            vec![Line::from(Span::styled(
                format!(
                    "  {} asking GitHub who is begging…",
                    SPINNER[(app.tick / 3) as usize % 4]
                ),
                muted(),
            ))]
        } else if app.filter.is_some() || (app.only_followed && app.screen == Screen::Board) {
            vec![Line::from(Span::styled(
                "  nothing matches (f changes the kind filter, w the followed-only filter)",
                muted(),
            ))]
        } else if app.screen == Screen::Board {
            vec![
                Line::from(Span::styled(
                    "  Nobody is begging right now.",
                    Style::new().bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  Ask for a review kind on a PR with:",
                    muted(),
                )),
                Line::from("    pr-begger request <pr-url> --kind rubber-stamp|ai|design"),
            ]
        } else if app.screen == Screen::Following && app.follows.is_empty() {
            vec![
                Line::from(Span::styled(
                    "  You don't follow anyone yet.",
                    Style::new().bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "  p  add people or teams (omril1, Autofleet/dev-ordering)",
                    muted(),
                )),
                Line::from(Span::styled("  +  on any PR follows its author", muted())),
            ]
        } else if app.screen == Screen::Following {
            vec![Line::from(Span::styled(
                "  No open PRs from the people you follow.",
                muted(),
            ))]
        } else {
            vec![Line::from(Span::styled(
                "  You have no open PRs in scope.",
                muted(),
            ))]
        };
        f.render_widget(Paragraph::new(msg), area);
        return (height, 0);
    }
    let width = area.width as usize;
    let cols = columns(app, &items, width);
    let rows = rows(app, &items, width);
    let cursor = app.cursor();
    let sel_line = rows
        .iter()
        .position(|r| matches!(r, Row::Item(i, _) if *i == cursor))
        .unwrap_or(0);
    let idx = app.screen.idx();
    let mut scroll = app.scroll[idx];
    let margin = 1.min(height.saturating_sub(1) / 2);
    if sel_line < scroll + margin {
        scroll = sel_line.saturating_sub(margin);
    }
    if sel_line + margin >= scroll + height {
        scroll = sel_line + margin + 1 - height;
    }
    scroll = scroll.min(rows.len().saturating_sub(height));
    let lines: Vec<Line> = rows
        .iter()
        .skip(scroll)
        .take(height)
        .map(|r| match r {
            Row::Header(l) => l.clone(),
            Row::Item(i, pr) => item_line(app, pr, &cols, *i == cursor),
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);
    (height, scroll)
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let Some(pr) = app.selected() else {
        f.render_widget(window("", area.width, pal::FADE), area);
        return;
    };
    let now = Utc::now();
    let color = pr.kind().map_or(pal::FADE, kind_color);
    let block = window(
        &format!("{}#{}", pr.short_repo(), pr.number),
        area.width,
        color,
    );
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 7 || inner.width < 20 {
        return;
    }

    pixel::icon(pr.kind()).render(inner.x + 1, inner.y + 1, inner, f.buffer_mut());
    let side = Rect::new(inner.x + 12, inner.y + 1, inner.width.saturating_sub(13), 5);
    let mut head = vec![];
    match &pr.request {
        Some(r) => {
            head.push(Line::from(Span::styled(
                r.kind.label(),
                Style::new().bold(),
            )));
            head.push(Line::from(Span::styled(r.kind.blurb(), muted())));
            head.push(Line::from(Span::styled(
                format!(
                    "asked by {} · {}",
                    r.by.as_deref().unwrap_or("you"),
                    age(r.at, now)
                ),
                muted(),
            )));
        }
        None => {
            head.push(Line::from(Span::styled("No kind yet", Style::new().bold())));
            head.push(Line::from(Span::styled("press c to beg for one", muted())));
        }
    }
    f.render_widget(Paragraph::new(head).wrap(Wrap { trim: true }), side);

    let body_area = Rect::new(
        inner.x + 1,
        inner.y + 7,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(7),
    );
    let mut lines = Vec::new();
    if let Some(why) = pr.request.as_ref().and_then(|r| r.why.as_deref()) {
        lines.push(Line::from(Span::styled(
            format!("“{why}”"),
            Style::new().italic(),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        pr.title.clone(),
        Style::new().bold(),
    )));
    lines.push(Line::from(Span::styled(pr.repo.clone(), muted())));
    lines.push(Line::from(""));
    let kv = |k: &str, v: Vec<Span<'static>>| {
        let mut s = vec![Span::styled(format!("{k:<9}"), muted())];
        s.extend(v);
        Line::from(s)
    };
    lines.push(kv("author", vec![Span::raw(pr.author.clone())]));
    lines.push(kv(
        "opened",
        vec![Span::raw(format!("{} ago", age(pr.created_at, now)))],
    ));
    lines.push(kv(
        "size",
        vec![Span::raw(format!("+{} −{}", pr.additions, pr.deletions))],
    ));
    lines.push(kv(
        "ci",
        vec![ci_span(pr.ci), Span::raw(format!(" {}", pr.ci.word()))],
    ));
    lines.push(kv(
        "review",
        vec![review_span(pr), Span::raw(format!(" {}", pr.review.word()))],
    ));
    lines.push(kv("merge", vec![merge_span(pr.merge, 14)]));
    if let Some(a) = &pr.activity {
        let new = app.new_keys.contains(&pr.key());
        lines.push(kv(
            "threads",
            vec![Span::raw(format!(
                "{} unresolved of {}",
                a.unresolved_threads, a.total_threads
            ))],
        ));
        lines.push(kv("comments", vec![Span::raw(a.comments.to_string())]));
        if let Some(t) = a.last_by_others {
            lines.push(kv(
                "activity",
                vec![
                    Span::raw(format!("{} ago", age(t, now))),
                    if new {
                        Span::styled("  ● new", Style::new().fg(pal::ROSE))
                    } else {
                        Span::raw("")
                    },
                ],
            ));
        }
    }
    if pr.draft {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "draft: not ready for review yet",
            muted(),
        )));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body_area);
}

/// The status bar: keys (or the latest message) left, freshness right.
fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let key = |k: &str| Span::styled(format!(" {k}"), Style::new().bold());
    let txt = |t: &str| Span::styled(format!(" {t} "), muted());
    let mut spans = vec![
        key("↑↓"),
        txt("move"),
        key("⏎"),
        txt("open"),
        key("c"),
        txt("beg"),
        key("f"),
        txt("filter"),
        key("tab"),
        txt("screen"),
    ];
    match app.screen {
        Screen::Mine => spans.extend([key("x"), txt("seen")]),
        Screen::Board => spans.extend([key("w"), txt("followed")]),
        Screen::Following => spans.extend([key("+"), txt("follow")]),
    }
    spans.extend([key("p"), txt("people")]);
    spans.extend([
        key("r"),
        txt("refresh"),
        key("?"),
        txt("help"),
        key("q"),
        txt("quit"),
    ]);
    let left = match &app.status {
        Some(s) => Line::from(Span::styled(
            format!(" {}", s.text),
            if s.error {
                Style::new().fg(Color::Red)
            } else {
                Style::new()
            },
        )),
        None => Line::from(spans),
    };
    // freshness and scope on the right, only when they fit beside the left
    let right = status_line(app);
    let rw = right.width() as u16;
    let fits = left.width() as u16 + rw < area.width;
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(if fits { rw } else { 0 }),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(left), left_area);
    if fits {
        f.render_widget(Paragraph::new(right).right_aligned(), right_area);
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn draw_help(f: &mut Frame, area: Rect) {
    let r = centered(area, 66, 29);
    f.render_widget(Clear, r);
    let block = window("Help", r.width, pal::LILAC);
    let k = |key: &str, what: &str| {
        Line::from(vec![
            Span::styled(format!("  {key:<14}"), Style::new().bold()),
            Span::styled(what.to_string(), muted()),
        ])
    };
    let mut lines = vec![
        k("j/k ↑/↓", "move"),
        k("g/G pgup/pgdn", "top / bottom / page"),
        k("enter / o", "open the PR in your browser"),
        k("c / b", "beg: set or change the CR kind (posts as you)"),
        k("f / F", "filter by kind (cycles)"),
        k("tab / 1 2 3", "begging board / my PRs / following"),
        k("x", "mark a PR's activity as seen"),
        k("p", "people & teams you follow (a add, d remove)"),
        k("+", "follow the selected PR's author"),
        k("w", "board: only PRs by people you follow"),
        k("r", "refresh now"),
        k("q / esc", "quit"),
        Line::from(""),
    ];
    for kind in Kind::ALL {
        let mut l = vec![Span::raw("  ")];
        l.extend(marker(Some(kind)));
        l.push(Span::raw(format!(" {} — {}", kind.label(), kind.blurb())));
        lines.push(Line::from(l));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  ci "),
        ci_span(Ci::Passing),
        Span::raw(" pass "),
        ci_span(Ci::Failing),
        Span::raw(" fail "),
        ci_span(Ci::Running),
        Span::raw(" running "),
        ci_span(Ci::None),
        Span::raw(" none"),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  review "),
        Span::styled("✓", Style::new().fg(Color::Green)),
        Span::raw(" approved "),
        Span::styled("✗", Style::new().fg(Color::Red)),
        Span::raw(" changes "),
        Span::styled("○", Style::new().fg(Color::Yellow)),
        Span::raw(" waiting"),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  threads "),
        Span::raw("◆2/5"),
        Span::raw(" unresolved/total   "),
        Span::styled("●", Style::new().fg(pal::ROSE)),
        Span::raw(" new since last look"),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  any key closes", muted())));
    f.render_widget(Paragraph::new(lines).block(block), r);
}

fn draw_follows(f: &mut Frame, app: &App, ed: &FollowEditor, area: Rect) {
    let entries = app.follows.all();
    let h = (entries.len() as u16 + 9).clamp(12, 24);
    let r = centered(area, 60, h);
    f.render_widget(Clear, r);
    let block = window("People & teams you follow", r.width, pal::LILAC);
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut lines = vec![Line::from("")];
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  nobody yet — press a to add",
            muted(),
        )));
    }
    for (i, who) in entries.iter().enumerate() {
        let (tag, name) = match who {
            Who::User(u) => ("user", u.clone()),
            Who::Team { .. } => ("team", who.to_string()),
        };
        let mut line = Line::from(vec![
            Span::raw(if i == ed.cursor { " ▸ " } else { "   " }),
            Span::styled(format!("{tag}  "), muted()),
            Span::raw(name),
        ]);
        if i == ed.cursor && ed.input.is_none() {
            line = line.style(Style::new().fg(pal::INK).bg(pal::LILAC));
        }
        lines.push(line);
    }
    lines.push(Line::from(""));
    match &ed.input {
        Some(text) => {
            lines.push(Line::from(vec![
                Span::styled("   add: ", muted()),
                Span::raw(text.clone()),
                Span::styled("▏", Style::new().add_modifier(Modifier::SLOW_BLINK)),
            ]));
            lines.push(Line::from(Span::styled(
                "   login, org/team, teams/<team>, or a GitHub URL",
                muted(),
            )));
            lines.push(Line::from(Span::styled(
                "   enter add · esc cancel",
                muted(),
            )));
        }
        None => {
            lines.push(Line::from(Span::styled(
                if ed.checking {
                    "   checking with GitHub…"
                } else {
                    "   a add · d remove · ↑↓ move · esc close"
                },
                muted(),
            )));
            if let Some(p) = app.follow_file.path() {
                lines.push(Line::from(Span::styled(
                    format!("   {}", p.display()),
                    muted(),
                )));
            }
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_picker(f: &mut Frame, p: &Picker, area: Rect) {
    let r = centered(area, 70, 16);
    f.render_widget(Clear, r);
    let kind = Kind::ALL[p.choice];
    let block = window("Beg for a review", r.width, kind_color(kind));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{} ", p.pr.key()), muted()),
            Span::styled(p.pr.title.clone(), Style::new().bold()),
        ]),
        Line::from(""),
    ];
    for (i, k) in Kind::ALL.into_iter().enumerate() {
        let on = i == p.choice;
        let mut l = vec![Span::raw(if on { " ▸ " } else { "   " })];
        l.extend(marker(Some(k)));
        l.push(Span::raw(format!(" {:<14}", k.label())));
        l.push(Span::styled(k.blurb(), muted()));
        let mut line = Line::from(l);
        if on {
            line = line.style(Style::new().fg(pal::INK).bg(pal::LILAC));
        }
        lines.push(line);
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" why: ", muted()),
        Span::raw(p.why.clone()),
        Span::styled("▏", Style::new().add_modifier(Modifier::SLOW_BLINK)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ↑↓ kind · type a reason (optional) · enter post · esc cancel",
        muted(),
    )));
    lines.push(Line::from(Span::styled(
        " posts (or updates your existing) request comment on the PR, as you",
        muted(),
    )));
    let text_area = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(1),
    );
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_truncates_and_pads_by_display_width() {
        assert_eq!(fit("abc", 5), "abc  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("日本語テキスト", 5).width(), 5);
        assert_eq!(fit("x", 0), "");
        assert_eq!(rfit("7", 3), "  7");
    }
}
