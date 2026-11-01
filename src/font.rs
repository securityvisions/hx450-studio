//! A tiny 5x7 pixel font.
//!
//! The panel is 160x96, so a bitmap font is both the honest choice and the one
//! that stays legible. Lowercase is folded to uppercase; unknown characters
//! render as a filled box.

use std::collections::HashMap;
use std::sync::OnceLock;

pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;

type Rows = [&'static str; GLYPH_H];

#[rustfmt::skip]
static GLYPHS: &[(char, Rows)] = &[
    (' ', ["     ", "     ", "     ", "     ", "     ", "     ", "     "]),
    ('!', ["  #  ", "  #  ", "  #  ", "  #  ", "     ", "     ", "  #  "]),
    ('"', [" # # ", " # # ", "     ", "     ", "     ", "     ", "     "]),
    ('#', [" # # ", "#####", " # # ", " # # ", "#####", " # # ", "     "]),
    ('%', ["##  #", "##  #", "   # ", "  #  ", " #   ", "#  ##", "   ##"]),
    ('(', ["   # ", "  #  ", " #   ", " #   ", " #   ", "  #  ", "   # "]),
    (')', [" #   ", "  #  ", "   # ", "   # ", "   # ", "  #  ", " #   "]),
    ('*', ["     ", "# # #", " ### ", "#####", " ### ", "# # #", "     "]),
    ('+', ["     ", "  #  ", "  #  ", "#####", "  #  ", "  #  ", "     "]),
    (',', ["     ", "     ", "     ", "     ", "     ", "  #  ", " #   "]),
    ('-', ["     ", "     ", "     ", "#####", "     ", "     ", "     "]),
    ('.', ["     ", "     ", "     ", "     ", "     ", "     ", "  #  "]),
    ('/', ["    #", "    #", "   # ", "  #  ", " #   ", "#    ", "#    "]),
    ('0', [" ### ", "#   #", "#  ##", "# # #", "##  #", "#   #", " ### "]),
    ('1', ["  #  ", " ##  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "]),
    ('2', [" ### ", "#   #", "    #", "   # ", "  #  ", " #   ", "#####"]),
    ('3', ["#####", "   # ", "  #  ", "   # ", "    #", "#   #", " ### "]),
    ('4', ["   # ", "  ## ", " # # ", "#  # ", "#####", "   # ", "   # "]),
    ('5', ["#####", "#    ", "#### ", "    #", "    #", "#   #", " ### "]),
    ('6', ["  ## ", " #   ", "#    ", "#### ", "#   #", "#   #", " ### "]),
    ('7', ["#####", "    #", "   # ", "  #  ", " #   ", " #   ", " #   "]),
    ('8', [" ### ", "#   #", "#   #", " ### ", "#   #", "#   #", " ### "]),
    ('9', [" ### ", "#   #", "#   #", " ####", "    #", "   # ", " ##  "]),
    (':', ["     ", "  #  ", "     ", "     ", "  #  ", "     ", "     "]),
    ('<', ["   # ", "  #  ", " #   ", "#    ", " #   ", "  #  ", "   # "]),
    ('=', ["     ", "     ", "#####", "     ", "#####", "     ", "     "]),
    ('>', [" #   ", "  #  ", "   # ", "    #", "   # ", "  #  ", " #   "]),
    ('?', [" ### ", "#   #", "    #", "   # ", "  #  ", "     ", "  #  "]),
    ('@', [" ### ", "#   #", "    #", " ## #", "# # #", "# # #", " ### "]),
    ('A', [" ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
    ('B', ["#### ", "#   #", "#   #", "#### ", "#   #", "#   #", "#### "]),
    ('C', [" ####", "#    ", "#    ", "#    ", "#    ", "#    ", " ####"]),
    ('D', ["#### ", "#   #", "#   #", "#   #", "#   #", "#   #", "#### "]),
    ('E', ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####"]),
    ('F', ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#    "]),
    ('G', [" ####", "#    ", "#    ", "#  ##", "#   #", "#   #", " ####"]),
    ('H', ["#   #", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
    ('I', [" ### ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "]),
    ('J', ["   ##", "    #", "    #", "    #", "#   #", "#   #", " ### "]),
    ('K', ["#   #", "#  # ", "# #  ", "##   ", "# #  ", "#  # ", "#   #"]),
    ('L', ["#    ", "#    ", "#    ", "#    ", "#    ", "#    ", "#####"]),
    ('M', ["#   #", "## ##", "# # #", "#   #", "#   #", "#   #", "#   #"]),
    ('N', ["#   #", "##  #", "# # #", "#  ##", "#   #", "#   #", "#   #"]),
    ('O', [" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]),
    ('P', ["#### ", "#   #", "#   #", "#### ", "#    ", "#    ", "#    "]),
    ('Q', [" ### ", "#   #", "#   #", "#   #", "# # #", "#  # ", " ## #"]),
    ('R', ["#### ", "#   #", "#   #", "#### ", "# #  ", "#  # ", "#   #"]),
    ('S', [" ####", "#    ", "#    ", " ### ", "    #", "    #", "#### "]),
    ('T', ["#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('U', ["#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]),
    ('V', ["#   #", "#   #", "#   #", "#   #", "#   #", " # # ", "  #  "]),
    ('W', ["#   #", "#   #", "#   #", "#   #", "# # #", "## ##", "#   #"]),
    ('X', ["#   #", "#   #", " # # ", "  #  ", " # # ", "#   #", "#   #"]),
    ('Y', ["#   #", "#   #", " # # ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('Z', ["#####", "    #", "   # ", "  #  ", " #   ", "#    ", "#####"]),
    ('[', ["  ###", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  ###"]),
    (']', ["###  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "###  "]),
    ('_', ["     ", "     ", "     ", "     ", "     ", "     ", "#####"]),
    ('|', ["  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('~', ["     ", "     ", " ## #", "#  # ", "     ", "     ", "     "]),
    ('°', [" ##  ", "#  # ", " ##  ", "     ", "     ", "     ", "     "]),
    ('■', ["#####", "#####", "#####", "#####", "#####", "#####", "#####"]),
];

fn table() -> &'static HashMap<char, [bool; GLYPH_W * GLYPH_H]> {
    static TABLE: OnceLock<HashMap<char, [bool; GLYPH_W * GLYPH_H]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::new();
        for (ch, rows) in GLYPHS {
            let mut bits = [false; GLYPH_W * GLYPH_H];
            for (y, row) in rows.iter().enumerate() {
                for (x, cell) in row.chars().enumerate().take(GLYPH_W) {
                    bits[y * GLYPH_W + x] = cell == '#';
                }
            }
            map.insert(*ch, bits);
        }
        map
    })
}

/// Width in pixels of `text` at the given scale (1 = one device pixel per font pixel).
pub fn text_width(text: &str, scale: usize, tracking: usize) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.chars().count() * (GLYPH_W * scale + tracking * scale) - tracking * scale
}

/// Draw `text` into an RGB buffer, returns the x after the last glyph.
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    buf: &mut [[u8; 3]],
    width: usize,
    height: usize,
    x: isize,
    y: isize,
    text: &str,
    scale: usize,
    color: [u8; 3],
    tracking: usize,
) -> isize {
    let font = table();
    let mut pen = x;
    for ch in text.chars() {
        let key = ch.to_ascii_uppercase();
        let bits = font.get(&key).or_else(|| font.get(&'■'));
        if let Some(bits) = bits {
            for gy in 0..GLYPH_H {
                for gx in 0..GLYPH_W {
                    if !bits[gy * GLYPH_W + gx] {
                        continue;
                    }
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = pen + (gx * scale + sx) as isize;
                            let py = y + (gy * scale + sy) as isize;
                            if px < 0 || py < 0 || px >= width as isize || py >= height as isize {
                                continue;
                            }
                            buf[py as usize * width + px as usize] = color;
                        }
                    }
                }
            }
        }
        pen += (GLYPH_W * scale + tracking * scale) as isize;
    }
    pen
}

pub fn draw_text_centered(
    buf: &mut [[u8; 3]],
    width: usize,
    height: usize,
    y: isize,
    text: &str,
    scale: usize,
    color: [u8; 3],
    tracking: usize,
) {
    let w = text_width(text, scale, tracking) as isize;
    let x = (width as isize - w) / 2;
    draw_text(buf, width, height, x, y, text, scale, color, tracking);
}
