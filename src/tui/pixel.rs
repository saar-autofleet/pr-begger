//! Pixel art: a soft pastel palette, a 5px block font and kind sprites,
//! drawn two pixels per terminal cell with half-block characters.
//!
//! Sprites get a one-pixel dark outline, the way 90s icons did, so the
//! pastels still read on light terminals; text elsewhere uses the terminal's
//! own colours.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::kind::Kind;

/// Pastels, plus an ink and an outline dark enough to sit under them.
pub mod pal {
    use ratatui::style::Color;
    pub const LILAC: Color = Color::Rgb(200, 182, 226);
    pub const PINK: Color = Color::Rgb(244, 182, 194);
    pub const ROSE: Color = Color::Rgb(226, 146, 168);
    pub const PEACH: Color = Color::Rgb(255, 209, 170);
    pub const BUTTER: Color = Color::Rgb(250, 227, 150);
    pub const MINT: Color = Color::Rgb(170, 222, 190);
    pub const SKY: Color = Color::Rgb(167, 199, 231);
    pub const CREAM: Color = Color::Rgb(255, 248, 231);
    pub const FADE: Color = Color::Rgb(160, 150, 180);
    pub const OUTLINE: Color = Color::Rgb(92, 84, 112);
    /// Text on pastel backgrounds.
    pub const INK: Color = Color::Rgb(45, 40, 60);
}

/// Each kind's signature pastel.
pub fn kind_color(kind: Kind) -> Color {
    match kind {
        Kind::RubberStamp => pal::MINT,
        Kind::Ai => pal::SKY,
        Kind::Design => pal::PEACH,
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
/// pastel gradient, top to bottom.
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
                out.set(x, y + 1, Some(pal::OUTLINE));
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
        0 | 1 => pal::SKY,
        2 => pal::LILAC,
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

const MINI_STAMP: [&str; 6] = [
    ".hhhh.", //
    "..hh..", //
    "..hh..", //
    ".rrrr.", //
    "rRRRRr", //
    "g.g.g.", //
];

const MINI_ROBOT: [&str; 6] = [
    "..y...", //
    ".bbbb.", //
    "bwbbwb", //
    "bbbbbb", //
    ".bssb.", //
    ".b..b.", //
];

const MINI_COMPASS: [&str; 6] = [
    "..oo..", //
    "..YY..", //
    "..ll..", //
    ".l..l.", //
    ".l..l.", //
    "s....p", //
];

const QUESTION: [&str; 8] = [
    "..ssss..", //
    ".ss..ss.", //
    ".....ss.", //
    "....ss..", //
    "...ss...", //
    "...ss...", //
    "........", //
    "...ss...", //
];

fn sprite_color(ch: char) -> Option<Color> {
    Some(match ch {
        'h' => pal::PEACH,
        'H' => pal::CREAM,
        'r' => pal::ROSE,
        'R' => pal::PINK,
        'g' => pal::MINT,
        'y' => pal::BUTTER,
        's' => pal::LILAC,
        'b' => pal::SKY,
        'w' => pal::CREAM,
        'o' => pal::PEACH,
        'Y' => pal::BUTTER,
        'l' => pal::LILAC,
        'p' => pal::PINK,
        _ => return None,
    })
}

/// The kind's 8×8 sprite, without outline.
pub fn sprite(kind: Kind) -> Bitmap {
    let rows: &[&str] = match kind {
        Kind::RubberStamp => &STAMP,
        Kind::Ai => &ROBOT,
        Kind::Design => &COMPASS,
    };
    Bitmap::from_rows(rows, sprite_color)
}

/// Surround every lit pixel with a one-pixel outline (4-neighbour), growing
/// the bitmap by one pixel on each side.
pub fn outlined(b: &Bitmap, color: Color) -> Bitmap {
    let mut out = Bitmap::new(b.w + 2, b.h + 2);
    for y in 0..out.h {
        for x in 0..out.w {
            let lit = |dx: isize, dy: isize| {
                let (sx, sy) = (x as isize - 1 + dx, y as isize - 1 + dy);
                sx >= 0 && sy >= 0 && b.get(sx as usize, sy as usize).is_some()
            };
            if lit(0, 0) {
                out.set(x, y, b.get(x - 1, y - 1));
            } else if lit(-1, 0) || lit(1, 0) || lit(0, -1) || lit(0, 1) {
                out.set(x, y, Some(color));
            }
        }
    }
    out
}

/// Outlined 8×8 kind icon: 10 columns × 5 rows. `None` is the "?" icon.
pub fn icon(kind: Option<Kind>) -> Bitmap {
    let s = match kind {
        Some(k) => sprite(k),
        None => Bitmap::from_rows(&QUESTION, sprite_color),
    };
    outlined(&s, pal::OUTLINE)
}

/// Outlined 6×6 kind icon for the header: 8 columns × 4 rows.
pub fn mini(kind: Kind) -> Bitmap {
    let rows: &[&str] = match kind {
        Kind::RubberStamp => &MINI_STAMP,
        Kind::Ai => &MINI_ROBOT,
        Kind::Design => &MINI_COMPASS,
    };
    outlined(&Bitmap::from_rows(rows, sprite_color), pal::OUTLINE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprites_are_8x8() {
        for k in Kind::ALL {
            let s = sprite(k);
            assert_eq!((s.w, s.h), (8, 8), "{k}");
            assert_eq!(icon(Some(k)).cells(), (10, 5));
            assert_eq!(mini(k).cells(), (8, 4));
        }
    }

    #[test]
    fn sprite_rows_are_rectangular() {
        for rows in [STAMP, ROBOT, COMPASS, QUESTION] {
            assert!(rows.iter().all(|r| r.chars().count() == 8));
        }
        for rows in [MINI_STAMP, MINI_ROBOT, MINI_COMPASS] {
            assert!(rows.iter().all(|r| r.chars().count() == 6));
        }
    }

    #[test]
    #[ignore]
    fn print_banner() {
        let b = banner("PR BEGGER");
        for y in 0..b.h {
            let row: String = (0..b.w)
                .map(|x| match b.get(x, y) {
                    Some(pal::OUTLINE) => '+',
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
            'a' => Some(pal::ROSE),
            'b' => Some(pal::SKY),
            _ => None,
        });
        let area = Rect::new(0, 0, 2, 1);
        let mut buf = Buffer::empty(area);
        b.render(0, 0, area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "█");
        assert_eq!(buf[(1, 0)].symbol(), "▀");
        assert_eq!(buf[(1, 0)].fg, pal::SKY);
    }

    #[test]
    fn outline_wraps_lit_pixels() {
        let b = Bitmap::from_rows(&["#"], |_| Some(pal::MINT));
        let o = outlined(&b, pal::OUTLINE);
        assert_eq!((o.w, o.h), (3, 3));
        assert_eq!(o.get(1, 1), Some(pal::MINT));
        assert_eq!(o.get(0, 1), Some(pal::OUTLINE));
        assert_eq!(o.get(1, 0), Some(pal::OUTLINE));
        assert_eq!(o.get(0, 0), None, "corners stay clear (4-neighbour)");
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
