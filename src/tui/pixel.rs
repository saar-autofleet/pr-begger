//! Pixel art: a retro palette, a 5px block font and kind sprites, drawn two
//! pixels per terminal cell with half-block characters.
//!
//! Sprites sit on an opaque dark tile, so they look the same on dark and
//! light terminals; text elsewhere uses the terminal's own colours.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::kind::Kind;

/// A 16-colour retro palette (the PICO-8 one; colours are not code, but
/// credit where due: Lexaloffle's fantasy console made it famous).
pub mod pal {
    use ratatui::style::Color;
    pub const INK: Color = Color::Rgb(29, 43, 83);
    pub const PLUM: Color = Color::Rgb(126, 37, 83);
    pub const RUST: Color = Color::Rgb(171, 82, 54);
    pub const SLATE: Color = Color::Rgb(95, 87, 79);
    pub const SILVER: Color = Color::Rgb(194, 195, 199);
    pub const CREAM: Color = Color::Rgb(255, 241, 232);
    pub const RED: Color = Color::Rgb(255, 0, 77);
    pub const ORANGE: Color = Color::Rgb(255, 163, 0);
    pub const YELLOW: Color = Color::Rgb(255, 236, 39);
    pub const GREEN: Color = Color::Rgb(0, 228, 54);
    pub const BLUE: Color = Color::Rgb(41, 173, 255);
    pub const LAVENDER: Color = Color::Rgb(131, 118, 156);
    pub const PINK: Color = Color::Rgb(255, 119, 168);
    pub const PEACH: Color = Color::Rgb(255, 204, 170);
    pub const BLACK: Color = Color::Rgb(0, 0, 0);
}

/// Each kind's signature colour, used for chips, headers and borders.
pub fn kind_color(kind: Kind) -> Color {
    match kind {
        Kind::RubberStamp => pal::GREEN,
        Kind::Ai => pal::BLUE,
        Kind::Design => pal::ORANGE,
    }
}

/// A grid of optional pixels; `None` is transparent.
#[derive(Debug, Clone, PartialEq)]
pub struct Bitmap {
    pub w: usize,
    pub h: usize,
    px: Vec<Option<Color>>,
}

impl Bitmap {
    pub fn new(w: usize, h: usize) -> Self {
        Bitmap {
            w,
            h,
            px: vec![None; w * h],
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Option<Color> {
        if x < self.w && y < self.h {
            self.px[y * self.w + x]
        } else {
            None
        }
    }

    pub fn set(&mut self, x: usize, y: usize, c: Option<Color>) {
        if x < self.w && y < self.h {
            self.px[y * self.w + x] = c;
        }
    }

    /// Build from rows of characters and a colour lookup (`None` = clear).
    pub fn from_rows(rows: &[&str], color: impl Fn(char) -> Option<Color>) -> Self {
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let mut b = Bitmap::new(w, rows.len());
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                b.set(x, y, color(ch));
            }
        }
        b
    }

    /// Terminal cells this needs: one column per pixel, one row per two.
    pub fn cells(&self) -> (u16, u16) {
        (self.w as u16, self.h.div_ceil(2) as u16)
    }

    /// Paint at `(x, y)`, clipped to `area`. Transparent pixels leave the
    /// cell's existing background alone.
    pub fn render(&self, x: u16, y: u16, area: Rect, buf: &mut Buffer) {
        let (_, rows) = self.cells();
        for ty in 0..rows {
            for px in 0..self.w as u16 {
                let (cx, cy) = (x + px, y + ty);
                if cx < area.x || cy < area.y || cx >= area.right() || cy >= area.bottom() {
                    continue;
                }
                let top = self.get(px as usize, ty as usize * 2);
                let bot = self.get(px as usize, ty as usize * 2 + 1);
                let Some(cell) = buf.cell_mut((cx, cy)) else {
                    continue;
                };
                match (top, bot) {
                    (None, None) => {}
                    (Some(t), None) => {
                        cell.set_char('▀').set_fg(t);
                    }
                    (None, Some(b)) => {
                        cell.set_char('▄').set_fg(b);
                    }
                    (Some(t), Some(b)) if t == b => {
                        cell.set_char('█').set_fg(t);
                    }
                    (Some(t), Some(b)) => {
                        cell.set_char('▀').set_fg(t).set_bg(b);
                    }
                }
            }
        }
    }
}

/// 5px-tall block font, just the letters the title needs plus a few spare.
fn glyph(ch: char) -> &'static [&'static str; 5] {
    match ch {
        'P' => &["###.", "#..#", "###.", "#...", "#..."],
        'R' => &["###.", "#..#", "###.", "#.#.", "#..#"],
        'B' => &["###.", "#..#", "###.", "#..#", "###."],
        'E' => &["####", "#...", "###.", "#...", "####"],
        'G' => &[".###", "#...", "#.##", "#..#", ".###"],
        'Y' => &["#..#", "#..#", ".##.", "..#.", "..#."],
        'M' => &["#...#", "##.##", "#.#.#", "#...#", "#...#"],
        'S' => &[".###", "#...", ".##.", "...#", "###."],
        ' ' => &["..", "..", "..", "..", ".."],
        _ => &["####", "#..#", "#..#", "#..#", "####"],
    }
}

/// Render `text` in the block font with a one-pixel drop shadow below and a
/// sunset gradient, top to bottom.
pub fn banner(text: &str) -> Bitmap {
    let glyphs: Vec<_> = text.chars().map(glyph).collect();
    let width: usize = glyphs.iter().map(|g| g[0].len()).sum::<usize>() + glyphs.len();
    let mut face = Bitmap::new(width, 5);
    let mut x0 = 0;
    for g in &glyphs {
        for (y, row) in g.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if ch == '#' {
                    face.set(x0 + x, y, Some(gradient(y)));
                }
            }
        }
        x0 += g[0].len() + 1;
    }
    let mut out = Bitmap::new(width, 6);
    for y in 0..5 {
        for x in 0..width {
            if face.get(x, y).is_some() {
                out.set(x, y + 1, Some(pal::PLUM));
            }
        }
    }
    for y in 0..5 {
        for x in 0..width {
            if let Some(c) = face.get(x, y) {
                out.set(x, y, Some(c));
            }
        }
    }
    out
}

fn gradient(y: usize) -> Color {
    match y {
        0 | 1 => pal::YELLOW,
        2 => pal::ORANGE,
        _ => pal::PINK,
    }
}

const STAMP: [&str; 8] = [
    "..hhhh..", //
    ".hHHHHh.", //
    "..hhhh..", //
    "...hh...", //
    ".rrrrrr.", //
    "rRRRRRRr", //
    "........", //
    "gg.gg.gg", //
];

const ROBOT: [&str; 8] = [
    "...yy...", //
    "...ss...", //
    ".bbbbbb.", //
    ".bwbbwb.", //
    ".bbbbbb.", //
    ".bbssbb.", //
    "..bbbb..", //
    ".bb..bb.", //
];

const COMPASS: [&str; 8] = [
    "...oo...", //
    "..oYYo..", //
    "...oo...", //
    "...ll...", //
    "..l..l..", //
    ".l....l.", //
    ".l....l.", //
    "s......p", //
];

fn sprite_color(ch: char) -> Option<Color> {
    Some(match ch {
        'h' => pal::RUST,
        'H' => pal::PEACH,
        'r' => pal::RED,
        'R' => pal::PINK,
        'g' => pal::GREEN,
        'y' => pal::YELLOW,
        's' => pal::SILVER,
        'b' => pal::BLUE,
        'w' => pal::CREAM,
        'o' => pal::ORANGE,
        'Y' => pal::YELLOW,
        'l' => pal::LAVENDER,
        'p' => pal::PINK,
        'k' => pal::BLACK,
        _ => return None,
    })
}

/// The kind's 8×8 sprite.
pub fn sprite(kind: Kind) -> Bitmap {
    let rows: &[&str] = match kind {
        Kind::RubberStamp => &STAMP,
        Kind::Ai => &ROBOT,
        Kind::Design => &COMPASS,
    };
    Bitmap::from_rows(rows, sprite_color)
}

/// A sprite on a 10×10 dark tile with a one-pixel frame in `frame`.
/// Renders as 10 columns × 5 rows.
pub fn tile(kind: Kind, frame: Color) -> Bitmap {
    let s = sprite(kind);
    let mut t = Bitmap::new(s.w + 2, s.h + 2);
    for y in 0..t.h {
        for x in 0..t.w {
            let edge = x == 0 || y == 0 || x == t.w - 1 || y == t.h - 1;
            t.set(x, y, Some(if edge { frame } else { pal::INK }));
        }
    }
    for y in 0..s.h {
        for x in 0..s.w {
            if let Some(c) = s.get(x, y) {
                t.set(x + 1, y + 1, Some(c));
            }
        }
    }
    t
}

/// A "?" tile for PRs that never said what they need.
pub fn unknown_tile(frame: Color) -> Bitmap {
    let rows = [
        "..ssss..", //
        ".ss..ss.", //
        ".....ss.", //
        "....ss..", //
        "...ss...", //
        "...ss...", //
        "........", //
        "...ss...", //
    ];
    let s = Bitmap::from_rows(&rows, sprite_color);
    let mut t = Bitmap::new(10, 10);
    for y in 0..10 {
        for x in 0..10 {
            let edge = x == 0 || y == 0 || x == 9 || y == 9;
            t.set(x, y, Some(if edge { frame } else { pal::INK }));
        }
    }
    for y in 0..8 {
        for x in 0..8 {
            if let Some(c) = s.get(x, y) {
                t.set(x + 1, y + 1, Some(c));
            }
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprites_are_8x8() {
        for k in Kind::ALL {
            let s = sprite(k);
            assert_eq!((s.w, s.h), (8, 8), "{k}");
            assert_eq!(tile(k, pal::SLATE).cells(), (10, 5));
        }
    }

    #[test]
    fn sprite_rows_are_rectangular() {
        for rows in [STAMP, ROBOT, COMPASS] {
            assert!(rows.iter().all(|r| r.chars().count() == 8));
        }
    }

    #[test]
    #[ignore]
    fn print_banner() {
        let b = banner("PR BEGGER");
        for y in 0..b.h {
            let row: String = (0..b.w)
                .map(|x| match b.get(x, y) {
                    Some(pal::PLUM) => '+',
                    Some(_) => '#',
                    None => '.',
                })
                .collect();
            println!("{row}");
        }
    }

    #[test]
    fn banner_fits_three_rows() {
        let b = banner("PR BEGGER");
        assert_eq!(b.cells().1, 3);
        assert!(b.w < 48, "{}", b.w);
    }

    #[test]
    fn renders_half_blocks() {
        let b = Bitmap::from_rows(&["ab", "a."], |c| match c {
            'a' => Some(pal::RED),
            'b' => Some(pal::BLUE),
            _ => None,
        });
        let area = Rect::new(0, 0, 2, 1);
        let mut buf = Buffer::empty(area);
        b.render(0, 0, area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "█");
        assert_eq!(buf[(1, 0)].symbol(), "▀");
        assert_eq!(buf[(1, 0)].fg, pal::BLUE);
    }

    #[test]
    fn render_clips() {
        let b = sprite(Kind::Ai);
        let area = Rect::new(0, 0, 3, 1);
        let mut buf = Buffer::empty(area);
        b.render(0, 0, area, &mut buf); // must not panic
        b.render(2, 0, area, &mut buf);
    }
}
