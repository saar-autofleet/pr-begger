//! Rendering. Everything here is a pure function of `App` plus the frame.

use chrono::Utc;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::pixel::{self, kind_color, pal};
use super::{App, Picker, Popup, Screen};
use crate::kind::Kind;
use crate::model::{age, Ci, Decision, Merge, Pr};

const SPINNER: [&str; 4] = ["▖", "▘", "▝", "▗"];
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

fn chip(kind: Option<Kind>) -> Span<'static> {
    match kind {
        Some(k) => Span::styled(
            format!(" {} ", k.tag()),
            Style::new().fg(pal::BLACK).bg(kind_color(k)).bold(),
        ),
        None => Span::styled("   ?   ", muted()),
    }
}

fn ci_span(ci: Ci) -> Span<'static> {
    match ci {
        Ci::Passing => Span::styled("✓", Style::new().fg(Color::Green)),
        Ci::Failing => Span::styled("✗", Style::new().fg(Color::Red).bold()),
        Ci::Running => Span::styled("◐", Style::new().fg(Color::Yellow)),
        Ci::None => Span::styled("·", muted()),
    }
}

fn review_span(pr: &Pr) -> Span<'static> {
    match pr.review.decision {
        Decision::Approved => Span::styled("✓", Style::new().fg(Color::Green)),
        Decision::ChangesRequested => Span::styled("✗", Style::new().fg(Color::Red).bold()),
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
        Merge::Conflict => ("conflict", Style::new().fg(Color::Red).bold()),
        Merge::Behind => ("behind", Style::new().fg(Color::Yellow)),
        Merge::Blocked => ("blocked", muted()),
        Merge::Unstable => ("unstable", Style::new().fg(Color::Yellow)),
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
        Constraint::Length(if tall { 5 } else { 1 }),
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
        None => {}
    }
    page
}

fn tabs(app: &App) -> Line<'static> {
    let new = app.new_keys.len();
    let tab = |label: String, on: bool| {
        if on {
            Span::styled(label, Style::new().fg(pal::BLACK).bg(pal::YELLOW).bold())
        } else {
            Span::styled(label, muted())
        }
    };
    let mut spans = vec![
        tab(
            format!(" 1 BEGGING {} ", app.board.len()),
            app.screen == Screen::Board,
        ),
        Span::raw(" "),
        tab(
            format!(" 2 MY PRS {} ", app.mine.len()),
            app.screen == Screen::Mine,
        ),
    ];
    if new > 0 {
        spans.push(Span::styled(
            format!(" ●{new} new"),
            Style::new().fg(Color::Yellow).bold(),
        ));
    }
    if let Some(k) = app.filter {
        spans.push(Span::raw("  filter "));
        spans.push(chip(Some(k)));
    }
    Line::from(spans)
}

fn status_line(app: &App) -> Line<'static> {
    let now = Utc::now();
    let at = match app.screen {
        Screen::Board => app.board_at,
        Screen::Mine => app.mine_at,
    };
    let mut spans = vec![Span::raw(" ")];
    if app.loading() {
        spans.push(Span::styled(
            format!(
                "{} fetching ",
                SPINNER[(app.tick / 3) as usize % SPINNER.len()]
            ),
            Style::new().fg(Color::Yellow),
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
    spans.push(Span::styled(app.scope_label(), muted()));
    Line::from(spans)
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let banner = pixel::banner("PR BEGGER");
    let (bw, _) = banner.cells();
    let left_w = (bw + 2).max(36).min(area.width);
    let [left, right] =
        Layout::horizontal([Constraint::Length(left_w), Constraint::Min(0)]).areas(area);
    banner.render(left.x + 1, left.y, left, f.buffer_mut());
    let [_, tabs_row, status_row] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(left);
    // tabs and status may run past the banner column into the tile area only
    // when there are no tiles; give them the full width and let tiles overdraw.
    let wide = |r: Rect| Rect {
        width: area.width,
        ..r
    };
    f.render_widget(Paragraph::new(tabs(app)), wide(tabs_row));
    f.render_widget(Paragraph::new(status_line(app)), wide(status_row));

    draw_tiles(f, app, right);
}

fn draw_compact_header(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled(
        "▙▟ PR BEGGER ",
        Style::new().fg(pal::ORANGE).bold(),
    )];
    spans.extend(tabs(app).spans);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// One sprite tile per kind with its count; the active filter is lit, the
/// others dim while a filter is on.
fn draw_tiles(f: &mut Frame, app: &App, area: Rect) {
    const TILE_W: u16 = 24;
    if area.width < TILE_W * 3 {
        return;
    }
    let list = match app.screen {
        Screen::Board => &app.board,
        Screen::Mine => &app.mine,
    };
    // Clear the tile region (tabs/status were drawn full width underneath).
    f.render_widget(Clear, area);
    for (i, k) in Kind::ALL.into_iter().enumerate() {
        let x = area.x + i as u16 * TILE_W;
        let r = Rect::new(x, area.y, TILE_W.min(area.right() - x), area.height);
        let lit = app.filter == Some(k);
        let dim = app.filter.is_some() && !lit;
        let frame = if lit {
            pal::YELLOW
        } else if dim {
            pal::SLATE
        } else {
            kind_color(k)
        };
        pixel::tile(k, frame).render(r.x, r.y, r, f.buffer_mut());
        let count = list.iter().filter(|p| p.kind() == Some(k)).count();
        let name_style = if dim {
            muted()
        } else {
            Style::new().fg(kind_color(k)).bold()
        };
        let text = vec![
            Line::from(Span::styled(
                match k {
                    Kind::RubberStamp => "RUBBER STAMP",
                    Kind::Ai => "AI REVIEW",
                    Kind::Design => "DESIGN",
                },
                name_style,
            )),
            Line::from(vec![
                Span::styled(format!("{count}"), Style::new().bold()),
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
        let tr = Rect::new(r.x + 11, r.y + 1, r.width.saturating_sub(11), 3);
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
    // mark + chip + gaps + age + ci + review
    let mut fixed = 1 + 7 + 1 + repo + 1 + 1 + 4 + 1 + 1 + 1 + 1;
    let mut author = 0;
    let mut size = 0;
    match app.screen {
        Screen::Board => {
            for (col, w) in [(&mut size, 11usize), (&mut author, 13usize)] {
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

fn group_header(kind: Option<Kind>, count: usize, width: usize) -> Line<'static> {
    let (label, blurb, color) = match kind {
        Some(k) => (k.label().to_uppercase(), k.blurb(), kind_color(k)),
        None => (
            "NO KIND YET".into(),
            "matched the search, no readable request",
            pal::LAVENDER,
        ),
    };
    let head = format!(" ▚▞ {label} · {count} ");
    let tail = format!(" {blurb} ");
    let rule = width.saturating_sub(head.width() + tail.width() + 1);
    Line::from(vec![
        Span::styled(head, Style::new().fg(pal::BLACK).bg(color).bold()),
        Span::styled("▔".repeat(rule.min(4)), Style::new().fg(color)),
        Span::styled(tail, muted()),
    ])
}

fn mine_header(new: bool, count: usize) -> Line<'static> {
    let (label, color) = if new {
        ("● NEW ACTIVITY", pal::YELLOW)
    } else {
        ("MY OPEN PRS", pal::LAVENDER)
    };
    Line::from(vec![
        Span::styled(
            format!(" ▚▞ {label} · {count} "),
            Style::new().fg(pal::BLACK).bg(color).bold(),
        ),
        Span::styled(
            if new {
                "  new since you last looked · x marks seen"
            } else {
                ""
            },
            muted(),
        ),
    ])
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
        Screen::Mine => {
            let mut last: Option<bool> = None;
            for (i, pr) in items.iter().enumerate() {
                let new = app.new_keys.contains(&pr.key());
                if last != Some(new) {
                    let n = items
                        .iter()
                        .filter(|p| app.new_keys.contains(&p.key()) == new)
                        .count();
                    out.push(Row::Header(mine_header(new, n)));
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
    let accent = pr.kind().map_or(pal::LAVENDER, kind_color);
    let mut spans = vec![
        Span::styled(if selected { "▌" } else { " " }, Style::new().fg(accent)),
        chip(pr.kind()),
        Span::raw(" "),
        Span::styled(
            fit(&format!("{}#{}", pr.short_repo(), pr.number), cols.repo),
            Style::new().fg(Color::Cyan),
        ),
        Span::raw(" "),
    ];
    let mut title_style = if pr.draft { muted() } else { Style::new() };
    if selected {
        title_style = title_style.bold();
    }
    if app.screen == Screen::Mine {
        let new = app.new_keys.contains(&pr.key());
        spans.push(Span::styled(
            if new { "● " } else { "  " },
            Style::new().fg(Color::Yellow).bold(),
        ));
    }
    spans.push(Span::styled(fit(&pr.title, cols.title), title_style));
    if cols.author > 0 {
        spans.push(Span::styled(
            fit(&format!(" @{}", pr.author), cols.author),
            Style::new().fg(Color::Magenta),
        ));
    }
    if cols.size > 0 {
        let add = format!("+{}", pr.additions);
        let del = format!("-{}", pr.deletions);
        let pad = cols.size.saturating_sub(add.width() + del.width() + 1);
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(add, Style::new().fg(Color::Green)));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(del, Style::new().fg(Color::Red)));
    }
    if app.screen == Screen::Mine {
        let a = pr.activity.clone().unwrap_or_default();
        let threads = if a.total_threads == 0 {
            Span::styled(rfit("–", 6), muted())
        } else if a.unresolved_threads > 0 {
            Span::styled(
                rfit(&format!("◆{}/{}", a.unresolved_threads, a.total_threads), 6),
                Style::new().fg(Color::Yellow).bold(),
            )
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
        line = line.style(Style::new().bg(Color::DarkGray));
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
                Style::new().fg(Color::Yellow),
            ))]
        } else if app.filter.is_some() {
            vec![Line::from(Span::styled(
                "  nothing with this kind (f to change filter)",
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
        let block = Block::bordered()
            .border_type(BorderType::QuadrantOutside)
            .border_style(Style::new().fg(pal::SLATE));
        f.render_widget(block, area);
        return;
    };
    let now = Utc::now();
    let color = pr.kind().map_or(pal::LAVENDER, kind_color);
    let block = Block::bordered()
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::new().fg(color))
        .title(Span::styled(
            format!(" {}#{} ", pr.short_repo(), pr.number),
            Style::new().fg(pal::BLACK).bg(color).bold(),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 6 || inner.width < 20 {
        return;
    }

    let tile = match pr.kind() {
        Some(k) => pixel::tile(k, color),
        None => pixel::unknown_tile(color),
    };
    tile.render(inner.x + 1, inner.y, inner, f.buffer_mut());
    let side = Rect::new(inner.x + 12, inner.y, inner.width.saturating_sub(12), 5);
    let mut head = vec![];
    match &pr.request {
        Some(r) => {
            head.push(Line::from(Span::styled(
                r.kind.label().to_uppercase(),
                Style::new().fg(color).bold(),
            )));
            head.push(Line::from(Span::styled(r.kind.blurb(), muted())));
            head.push(Line::from(Span::styled(
                format!(
                    "by {} · {}",
                    r.by.as_deref()
                        .map_or("you".to_string(), |b| format!("@{b}")),
                    age(r.at, now)
                ),
                muted(),
            )));
        }
        None => {
            head.push(Line::from(Span::styled(
                "NO KIND YET",
                Style::new().fg(color).bold(),
            )));
            head.push(Line::from(Span::styled("press c to beg for one", muted())));
        }
    }
    f.render_widget(Paragraph::new(head).wrap(Wrap { trim: true }), side);

    let body_area = Rect::new(
        inner.x + 1,
        inner.y + 6,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(6),
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
    lines.push(Line::from(Span::styled(
        pr.repo.clone(),
        Style::new().fg(Color::Cyan),
    )));
    lines.push(Line::from(""));
    let kv = |k: &str, v: Vec<Span<'static>>| {
        let mut s = vec![Span::styled(format!("{k:<9}"), muted())];
        s.extend(v);
        Line::from(s)
    };
    lines.push(kv(
        "author",
        vec![Span::styled(
            format!("@{}", pr.author),
            Style::new().fg(Color::Magenta),
        )],
    ));
    lines.push(kv(
        "opened",
        vec![Span::raw(format!("{} ago", age(pr.created_at, now)))],
    ));
    lines.push(kv(
        "size",
        vec![
            Span::styled(format!("+{}", pr.additions), Style::new().fg(Color::Green)),
            Span::raw(" "),
            Span::styled(format!("-{}", pr.deletions), Style::new().fg(Color::Red)),
        ],
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
                        Span::styled("  ● NEW", Style::new().fg(Color::Yellow).bold())
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

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some(s) = &app.status {
        let style = if s.error {
            Style::new().fg(Color::Red).bold()
        } else {
            Style::new().fg(Color::Green)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(format!(" {}", s.text), style))),
            area,
        );
        return;
    }
    let key = |k: &str| {
        Span::styled(
            format!(" {k} "),
            Style::new().fg(pal::BLACK).bg(pal::SILVER),
        )
    };
    let txt = |t: &str| Span::styled(format!(" {t}  "), muted());
    let mut spans = vec![
        key("↑↓"),
        txt("move"),
        key("⏎"),
        txt("open"),
        key("c"),
        txt("beg/change kind"),
        key("f"),
        txt("filter"),
        key("tab"),
        txt("screen"),
    ];
    if app.screen == Screen::Mine {
        spans.extend([key("x"), txt("seen")]);
    }
    spans.extend([
        key("r"),
        txt("refresh"),
        key("?"),
        txt("help"),
        key("q"),
        txt("quit"),
    ]);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
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
    let r = centered(area, 66, 26);
    f.render_widget(Clear, r);
    let block = Block::bordered()
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::new().fg(pal::YELLOW))
        .title(Span::styled(
            " HELP ",
            Style::new().fg(pal::BLACK).bg(pal::YELLOW).bold(),
        ));
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
        k("tab / 1 / 2", "begging board / my PRs"),
        k("x", "mark a PR's activity as seen"),
        k("r", "refresh now"),
        k("q / esc", "quit"),
        Line::from(""),
    ];
    for kind in Kind::ALL {
        lines.push(Line::from(vec![
            Span::raw("  "),
            chip(Some(kind)),
            Span::raw(format!(" {} — {}", kind.label(), kind.blurb())),
        ]));
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
        Span::styled("◆2/5", Style::new().fg(Color::Yellow).bold()),
        Span::raw(" unresolved/total   "),
        Span::styled("●", Style::new().fg(Color::Yellow).bold()),
        Span::raw(" new since last look"),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  any key closes", muted())));
    f.render_widget(Paragraph::new(lines).block(block), r);
}

fn draw_picker(f: &mut Frame, p: &Picker, area: Rect) {
    let r = centered(area, 70, 15);
    f.render_widget(Clear, r);
    let kind = Kind::ALL[p.choice];
    let block = Block::bordered()
        .border_type(BorderType::QuadrantOutside)
        .border_style(Style::new().fg(kind_color(kind)))
        .title(Span::styled(
            " BEG FOR A REVIEW ",
            Style::new().fg(pal::BLACK).bg(kind_color(kind)).bold(),
        ));
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{} ", p.pr.key()), Style::new().fg(Color::Cyan)),
            Span::styled(p.pr.title.clone(), Style::new().bold()),
        ]),
        Line::from(""),
    ];
    for (i, k) in Kind::ALL.into_iter().enumerate() {
        let on = i == p.choice;
        lines.push(Line::from(vec![
            Span::styled(
                if on { " ▶ " } else { "   " },
                Style::new().fg(kind_color(k)).bold(),
            ),
            chip(Some(k)),
            Span::styled(
                format!(" {:<14}", k.label()),
                if on {
                    Style::new().bold()
                } else {
                    Style::new()
                },
            ),
            Span::styled(k.blurb(), muted()),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" why: ", muted()),
        Span::raw(p.why.clone()),
        Span::styled(
            "▏",
            Style::new()
                .fg(kind_color(kind))
                .add_modifier(Modifier::SLOW_BLINK),
        ),
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
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
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
