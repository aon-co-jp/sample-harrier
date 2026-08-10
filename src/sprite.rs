//! 手描きのドット絵パターンをRGBA8テクスチャへ焼き込む(ユーザー指示、
//! 2026-08-10「PNGドット絵スプライト(事前生成)」への対応)。`font.rs`と
//! 同じ「文字→ドットパターン→RGBA8」という考え方を、キャラクター
//! スプライト(自機・敵・ボス)に適用したもの。
//!
//! パターンは1行1文字列の配列(全行同じ長さ)で表し、各文字を
//! `palette`関数でRGBA8色へマッピングする。`.`は背景色(スプライトの
//! 縁取り・下地)として扱う。実際の画像ファイル(PNG)を用意する代わりに、
//! このプログラム自身がドット絵を生成する——アセット制作ツールが無い
//! Phase 0の制約下での実務的な折衷案(正直な開示)。

use directx_graphics_vulkan::{Rgba8, TextureRgba8};

pub fn rasterize(pattern: &[&str], palette: impl Fn(char) -> Rgba8, scale: usize) -> TextureRgba8 {
    let rows = pattern.len();
    let cols = pattern[0].chars().count();
    let width = cols * scale;
    let height = rows * scale;
    let mut pixels = vec![palette('.'); width * height];

    for (row, line) in pattern.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let color = palette(ch);
            for dy in 0..scale {
                for dx in 0..scale {
                    let x = col * scale + dx;
                    let y = row * scale + dy;
                    pixels[y * width + x] = color;
                }
            }
        }
    }

    TextureRgba8 { width: width as u32, height: height as u32, pixels }
}

/// 自機(疑似3Dシューター用の小型戦闘機、独自デザイン)。
pub const SHIP: &[&str] = &[
    "......AA......",
    ".....AAAA.....",
    ".....AAAA.....",
    "....AAAAAA....",
    "...BBAAAABB...",
    "..BBBAAAABBB..",
    ".BBBBAAAABBBB.",
    "CCBBBB..BBBBCC",
    "..CC......CC..",
    "...C......C...",
];

/// 雑魚敵(疑似3Dシューター用の小型敵機、独自デザイン)。
pub const ENEMY: &[&str] = &[
    "....DDDDDD....",
    "..DDEEEEEEDD..",
    ".DDEEFFFFEEDD.",
    "DDEEFF..FFEEDD",
    "DEEFF....FFEED",
    "DEEFF....FFEED",
    "DDEEFF..FFEEDD",
    ".DDEEFFFFEEDD.",
    "..DD......DD..",
    "....D....D....",
];

/// ボス(大型敵機、独自デザイン)。
pub const BOSS: &[&str] = &[
    "......GGGGGG......",
    "....GGHHHHHHGG....",
    "...GHHIIIIIIHHG...",
    "..GHHIIJJJJIIHHG..",
    ".GHHIIJJ..JJIIHHG.",
    "GHHIIJJ....JJIIHHG",
    "GHHIIJJ....JJIIHHG",
    ".GHHIIJJ..JJIIHHG.",
    "..GHHIIJJJJIIHHG..",
    "...GHHIIIIIIHHG...",
    "....GG..GG..GG....",
    ".....G....G.......",
];

pub fn ship_palette(c: char) -> Rgba8 {
    match c {
        'A' => Rgba8 { r: 230, g: 230, b: 240, a: 255 },
        'B' => Rgba8 { r: 40, g: 90, b: 220, a: 255 },
        'C' => Rgba8 { r: 255, g: 150, b: 30, a: 255 },
        _ => Rgba8 { r: 20, g: 20, b: 30, a: 255 },
    }
}

pub fn enemy_palette(c: char) -> Rgba8 {
    match c {
        'D' => Rgba8 { r: 140, g: 20, b: 20, a: 255 },
        'E' => Rgba8 { r: 210, g: 30, b: 30, a: 255 },
        'F' => Rgba8 { r: 250, g: 200, b: 60, a: 255 },
        _ => Rgba8 { r: 20, g: 20, b: 30, a: 255 },
    }
}

pub fn boss_palette(c: char) -> Rgba8 {
    match c {
        'G' => Rgba8 { r: 90, g: 20, b: 110, a: 255 },
        'H' => Rgba8 { r: 140, g: 40, b: 160, a: 255 },
        'I' => Rgba8 { r: 190, g: 80, b: 210, a: 255 },
        'J' => Rgba8 { r: 255, g: 220, b: 60, a: 255 },
        _ => Rgba8 { r: 20, g: 20, b: 30, a: 255 },
    }
}
