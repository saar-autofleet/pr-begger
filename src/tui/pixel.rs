//! Pixel art: a soft pastel palette, a 5px block font and kind icons.
//!
//! Every pixel is one or two whole terminal cells painted with a background
//! colour. Background fills cover the full cell, including the gap many
//! terminals leave between lines, and involve no glyph anti-aliasing, so
//! the pixels stay crisp and square-edged in any terminal. (Half-block
//! glyphs pack twice the pixels but smear and stripe in terminals that do
//! not draw block characters themselves.)

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::kind::Kind;

/// Pastels, plus an ink dark enough to sit on them.
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
    // shades for pixel-art shading
    pub const PEACH_DARK: Color = Color::Rgb(214, 158, 116);
    pub const MINT_DARK: Color = Color::Rgb(108, 172, 136);
    pub const SKY_DARK: Color = Color::Rgb(108, 148, 198);
    pub const LILAC_DARK: Color = Color::Rgb(148, 128, 184);
    /// Pixel-art outline: a deep plum-grey, softer than black.
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
    /// Terminal columns per pixel: 2 gives square pixels (cells are about
    /// twice as tall as wide), 1 gives tall ones.
    pub px_w: u16,
    px: Vec<Option<Color>>,
}

impl Bitmap {
    pub fn new(w: usize, h: usize, px_w: u16) -> Self {
        Bitmap {
            w,
            h,
            px_w,
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
    pub fn from_rows(rows: &[&str], px_w: u16, color: impl Fn(char) -> Option<Color>) -> Self {
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let mut b = Bitmap::new(w, rows.len(), px_w);
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                b.set(x, y, color(ch));
            }
        }
        b
    }

    /// Terminal cells this covers.
    pub fn cells(&self) -> (u16, u16) {
        (self.w as u16 * self.px_w, self.h as u16)
    }

    /// Paint at `(x, y)`, clipped to `area`. Transparent pixels leave the
    /// cells underneath alone.
    pub fn render(&self, x: u16, y: u16, area: Rect, buf: &mut Buffer) {
        for py in 0..self.h {
            for px in 0..self.w {
                let Some(color) = self.get(px, py) else {
                    continue;
                };
                for dx in 0..self.px_w {
                    let (cx, cy) = (x + px as u16 * self.px_w + dx, y + py as u16);
                    if cx < area.x || cy < area.y || cx >= area.right() || cy >= area.bottom() {
                        continue;
                    }
                    if let Some(cell) = buf.cell_mut((cx, cy)) {
                        cell.set_char(' ').set_bg(color);
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
        ' ' => &["..", "..", "..", "..", ".."],
        _ => &["####", "#..#", "#..#", "#..#", "####"],
    }
}

/// `text` in the block font, one column per pixel (tall pixels, like DOS
/// block letters), with a pastel gradient top to bottom.
pub fn banner(text: &str) -> Bitmap {
    let glyphs: Vec<_> = text.chars().map(glyph).collect();
    let width = glyphs
        .iter()
        .map(|g| g[0].len() + 1)
        .sum::<usize>()
        .saturating_sub(1);
    let mut out = Bitmap::new(width, 5, 1);
    let mut x0 = 0;
    for g in &glyphs {
        for (y, row) in g.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if ch == '#' {
                    out.set(x0 + x, y, Some(gradient(y)));
                }
            }
        }
        x0 += g[0].len() + 1;
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

// Icons are drawn EGA-style: a dark outline (k), a base pastel, a lighter
// highlight and a darker shade per material. Upper case is the shade.
//
//   k outline      w cream highlight
//   h/H wood       r/R rubber (rose)    g/G ink (mint)
//   b/B robot      e eye glow           y bulb / pencil wood
//   o/O hinge      l/L legs (lilac)     p pencil (pink)   s steel

// Detail-window icons: 10×9 square pixels, 20 columns × 9 rows.

const STAMP: [&str; 9] = [
    "...kkkk...", //
    "..kwhhHk..", //
    "..khhhHk..", //
    "...khHk...", //
    "..kkhHkk..", //
    ".krrrrrRk.", //
    "kwrrrrrrRk", //
    ".kkkkkkkk.", //
    "g.gG.Gg.gG", //
];

const ROBOT: [&str; 9] = [
    "....ky....", //
    "....kk....", //
    ".kkkkkkkk.", //
    "kbwbbbbbBk", //
    "kbekbbekBk", //
    "kbbbbbbbBk", //
    "kbskskskBk", //
    ".kBBBBBBk.", //
    "..kk..kk..", //
];

const COMPASS: [&str; 9] = [
    "....kk....", //
    "...kwoOk..", //
    "...kOOOk..", //
    "....klk...", //
    "...kl.Lk..", //
    "..kl...Lk.", //
    "..kl...pk.", //
    ".kl.....yk", //
    ".s.......k", //
];

const QUESTION: [&str; 9] = [
    "..kkkkk...", //
    ".kwlllLk..", //
    ".kLk.kLk..", //
    "....klLk..", //
    "...klLk...", //
    "...kLk....", //
    "....k.....", //
    "...klk....", //
    "...kLk....", //
];

// Header legend icons: 6×5 square pixels, 12 columns × 5 rows.

const MINI_STAMP: [&str; 5] = [
    ".kwhHk", //
    "..khk.", //
    "krrrRk", //
    "kkkkkk", //
    "g.gG.g", //
];

const MINI_ROBOT: [&str; 5] = [
    "..ky..", //
    ".kkkk.", //
    "kebbeB", //
    "kbsssB", //
    ".kBBk.", //
];

const MINI_COMPASS: [&str; 5] = [
    "..kOk.", //
    "..klk.", //
    ".kl.Lk", //
    ".kl.pk", //
    ".s...y", //
];

fn sprite_color(ch: char) -> Option<Color> {
    Some(match ch {
        'k' => pal::OUTLINE,
        'w' => pal::CREAM,
        'h' | 'o' => pal::PEACH,
        'H' | 'O' => pal::PEACH_DARK,
        'r' => pal::PINK,
        'R' => pal::ROSE,
        'g' => pal::MINT,
        'G' => pal::MINT_DARK,
        'b' => pal::SKY,
        'B' => pal::SKY_DARK,
        'e' => pal::MINT,
        'y' => pal::BUTTER,
        'l' => pal::LILAC,
        'L' => pal::LILAC_DARK,
        'p' => pal::ROSE,
        's' => pal::FADE,
        _ => return None,
    })
}

/// The detail window's kind icon; `None` is a "?".
pub fn icon(kind: Option<Kind>) -> Bitmap {
    let rows: &[&str] = match kind {
        Some(Kind::RubberStamp) => &STAMP,
        Some(Kind::Ai) => &ROBOT,
        Some(Kind::Design) => &COMPASS,
        None => &QUESTION,
    };
    Bitmap::from_rows(rows, 2, sprite_color)
}

/// The header legend's small kind icon.
pub fn mini(kind: Kind) -> Bitmap {
    let rows: &[&str] = match kind {
        Kind::RubberStamp => &MINI_STAMP,
        Kind::Ai => &MINI_ROBOT,
        Kind::Design => &MINI_COMPASS,
    };
    Bitmap::from_rows(rows, 2, sprite_color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_sizes() {
        for k in Kind::ALL {
            assert_eq!(icon(Some(k)).cells(), (20, 9), "{k}");
            assert_eq!(mini(k).cells(), (12, 5), "{k}");
        }
        assert_eq!(icon(None).cells(), (20, 9));
    }

    #[test]
    fn sprite_rows_are_rectangular() {
        for rows in [STAMP, ROBOT, COMPASS, QUESTION] {
            assert!(rows.iter().all(|r| r.chars().count() == 10), "{rows:?}");
        }
        for rows in [MINI_STAMP, MINI_ROBOT, MINI_COMPASS] {
            assert!(rows.iter().all(|r| r.chars().count() == 6), "{rows:?}");
        }
        // every pixel letter has a colour
        for rows in [
            &STAMP[..],
            &ROBOT,
            &COMPASS,
            &QUESTION,
            &MINI_STAMP,
            &MINI_ROBOT,
            &MINI_COMPASS,
        ] {
            for ch in rows.iter().flat_map(|r| r.chars()) {
                assert!(
                    ch == '.' || sprite_color(ch).is_some(),
                    "no colour for {ch:?}"
                );
            }
        }
    }

    #[test]
    fn banner_is_five_rows() {
        let b = banner("PR BEGGER");
        assert_eq!(b.cells(), (42, 5));
    }

    #[test]
    #[ignore]
    fn print_banner() {
        let b = banner("PR BEGGER");
        for y in 0..b.h {
            let row: String = (0..b.w)
                .map(|x| if b.get(x, y).is_some() { '#' } else { '.' })
                .collect();
            println!("{row}");
        }
    }

    #[test]
    fn renders_solid_background_cells() {
        let b = Bitmap::from_rows(&["a.", ".b"], 2, |c| match c {
            'a' => Some(pal::ROSE),
            'b' => Some(pal::SKY),
            _ => None,
        });
        let area = Rect::new(0, 0, 4, 2);
        let mut buf = Buffer::empty(area);
        b.render(0, 0, area, &mut buf);
        for x in 0..2 {
            assert_eq!(buf[(x, 0)].symbol(), " ");
            assert_eq!(buf[(x, 0)].bg, pal::ROSE);
            assert_eq!(buf[(x + 2, 1)].bg, pal::SKY);
        }
        assert_eq!(buf[(2, 0)].bg, Color::Reset, "transparent pixels untouched");
    }

    #[test]
    fn render_clips() {
        let b = icon(Some(Kind::Ai));
        let area = Rect::new(0, 0, 3, 1);
        let mut buf = Buffer::empty(area);
        b.render(0, 0, area, &mut buf);
        b.render(2, 0, area, &mut buf);
    }
}
