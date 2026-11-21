use ab_glyph::{FontArc, PxScale};
use image::{Rgba, RgbaImage};
use imageproc::drawing::{draw_text_mut, text_size};

use crate::error::BotError;
use crate::models::{parse_hex_color, Cosmetics, OutlineMode, RarityTier};

pub const CARD_W: u32 = 360;
pub const CARD_H: u32 = 520;
const GAP: u32 = 16;

const FONT_BYTES: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");

#[derive(Clone)]
pub struct Fonts {
    pub regular: FontArc,
}

impl Fonts {
    pub fn load() -> Result<Self, BotError> {
        let regular = FontArc::try_from_slice(FONT_BYTES)
            .map_err(|err| BotError::user_message(format!("font failed to load: {err:?}")))?;
        Ok(Self { regular })
    }
}

#[derive(Debug, Clone)]
pub struct CardFace {
    pub character_name: String,
    pub series_name: String,
    pub series_id: String,
    pub rarity: String,
    pub variant: String,
    pub print_number: Option<i32>,
    pub card_uid: Option<String>,
    pub base_png: Option<Vec<u8>>,
    pub cosmetics: Cosmetics,
    pub slot_label: Option<u8>,
    pub claimed: bool,
}

pub fn render_card(fonts: &Fonts, face: &CardFace) -> Result<Vec<u8>, BotError> {
    encode_png(&compose_card(fonts, face))
}

pub fn render_drop_strip(fonts: &Fonts, faces: &[CardFace; 3]) -> Result<Vec<u8>, BotError> {
    let card_w = CARD_W;
    let width = card_w * 3 + GAP * 2;
    let mut strip = RgbaImage::from_pixel(width, CARD_H, Rgba([12, 10, 18, 255]));
    for (index, face) in faces.iter().enumerate() {
        let card = compose_card(fonts, face);
        blit(&mut strip, &card, (card_w + GAP) * index as u32, 0);
    }
    encode_png(&strip)
}

fn compose_card(fonts: &Fonts, face: &CardFace) -> RgbaImage {
    // z-order is fixed: base art, filter, frame, 3D edge, then typography.
    let mut img = base_layer(face);
    apply_filter(&mut img, face.cosmetics.filter_id.as_deref());
    paint_frame(&mut img, face.cosmetics.frame_id.as_deref(), &face.rarity);
    paint_effect(&mut img, face.cosmetics.effect_3d_id.as_deref());
    paint_footer(&mut img);
    paint_text(fonts, &mut img, face);
    if face.claimed {
        dim(&mut img);
        let scale = PxScale::from(42.0);
        draw_outlined(
            fonts,
            &mut img,
            "CLAIMED",
            78,
            230,
            scale,
            [255, 255, 255],
            OutlineMode::Black,
        );
    }
    img
}

fn base_layer(face: &CardFace) -> RgbaImage {
    if let Some(bytes) = &face.base_png {
        if let Ok(decoded) = image::load_from_memory(bytes) {
            return cover(&decoded.to_rgba8(), CARD_W, CARD_H);
        }
    }
    procedural(face)
}

fn procedural(face: &CardFace) -> RgbaImage {
    let mut img = RgbaImage::new(CARD_W, CARD_H);
    let seed = hash_str(&format!("{}:{}", face.series_id, face.character_name));
    let c1 = palette(seed);
    let c2 = palette(seed.rotate_left(11));
    for y in 0..CARD_H {
        let t = y as f32 / CARD_H as f32;
        for x in 0..CARD_W {
            let stripe = if ((x + y / 6) / 14) % 2 == 0 { 10 } else { 0 };
            let r = lerp(c1[0], c2[0], t) + stripe;
            let g = lerp(c1[1], c2[1], t);
            let b = lerp(c1[2], c2[2], t);
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }
    let cx = CARD_W / 2;
    let cy = CARD_H / 2 - 20;
    let radius = 92.0;
    for y in cy.saturating_sub(100)..(cy + 110).min(CARD_H) {
        for x in cx.saturating_sub(100)..(cx + 100).min(CARD_W) {
            let dx = x as f32 - cx as f32;
            let dy = y as f32 - cy as f32;
            if dx * dx + dy * dy <= radius * radius {
                blend_pixel(&mut img, x, y, Rgba([255, 255, 255, 36]));
            }
        }
    }
    img
}

fn apply_filter(img: &mut RgbaImage, filter_id: Option<&str>) {
    let Some(id) = filter_id else {
        return;
    };
    let kind = filter_kind(id);
    if kind == FilterKind::None {
        return;
    }
    let width = img.width();
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let [r, g, b, a] = pixel.0;
        let (r, g, b) = match kind {
            FilterKind::None => (r, g, b),
            FilterKind::Warm => (sat(r as i32 + 22), g, sat(b as i32 - 16)),
            FilterKind::Cool => (sat(r as i32 - 14), g, sat(b as i32 + 24)),
            FilterKind::Gray => {
                let y = ((r as u16) * 30 + (g as u16) * 59 + (b as u16) * 11) / 100;
                (y as u8, y as u8, y as u8)
            }
            FilterKind::Vivid => (
                sat((r as i32 - 128) * 13 / 10 + 128),
                sat((g as i32 - 128) * 13 / 10 + 128),
                sat((b as i32 - 128) * 13 / 10 + 128),
            ),
            FilterKind::Holo => {
                let shift = ((x + y) % 48) as i32;
                (
                    sat(r as i32 + shift - 16),
                    sat(g as i32 + 8),
                    sat(b as i32 + (24 - shift)),
                )
            }
        };
        let _ = width;
        *pixel = Rgba([r, g, b, a]);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FilterKind {
    None,
    Warm,
    Cool,
    Gray,
    Vivid,
    Holo,
}

fn filter_kind(id: &str) -> FilterKind {
    let id = id.to_ascii_lowercase();
    if id.contains("holo") || id.contains("shiny") {
        FilterKind::Holo
    } else if id.contains("warm") {
        FilterKind::Warm
    } else if id.contains("cool") {
        FilterKind::Cool
    } else if id.contains("gray") || id.contains("grey") {
        FilterKind::Gray
    } else if id.contains("vivid") {
        FilterKind::Vivid
    } else {
        match hash_str(&id) % 5 {
            0 => FilterKind::Warm,
            1 => FilterKind::Cool,
            2 => FilterKind::Gray,
            3 => FilterKind::Vivid,
            _ => FilterKind::Holo,
        }
    }
}

fn paint_frame(img: &mut RgbaImage, frame_id: Option<&str>, rarity: &str) {
    let color = frame_id
        .map(|id| palette(hash_str(id)))
        .unwrap_or_else(|| rarity_color(RarityTier::parse(rarity)));
    let rgba = Rgba([color[0], color[1], color[2], 255]);
    hollow_rect(img, 8, 8, CARD_W - 16, CARD_H - 16, 6, rgba);
    hollow_rect(
        img,
        18,
        18,
        CARD_W - 36,
        CARD_H - 36,
        2,
        Rgba([255, 255, 255, 90]),
    );
}

fn paint_effect(img: &mut RgbaImage, effect_id: Option<&str>) {
    let Some(id) = effect_id else {
        return;
    };
    let seed = hash_str(id);
    let cyan = Rgba([80, 220, 255, 170]);
    let magenta = Rgba([255, 80, 180, 150]);
    hollow_rect(img, 4, 6, CARD_W - 10, CARD_H - 10, 2, cyan);
    hollow_rect(img, 10, 4, CARD_W - 12, CARD_H - 12, 2, magenta);
    for n in 0..18 {
        let x = (seed.rotate_left(n) % (CARD_W - 24)) + 12;
        let y = (seed.rotate_left(n + 5) % (CARD_H - 24)) + 12;
        if x + y % 9 == 0 || n % 2 == 0 {
            fill_rect(img, x as i32, y as i32, 3, 3, Rgba([255, 255, 255, 180]));
        }
    }
}

fn paint_footer(img: &mut RgbaImage) {
    let top = CARD_H - 128;
    for y in top..CARD_H {
        for x in 0..CARD_W {
            blend_pixel(img, x, y, Rgba([10, 8, 18, 168]));
        }
    }
}

fn paint_text(fonts: &Fonts, img: &mut RgbaImage, face: &CardFace) {
    let fill = face
        .cosmetics
        .text_color_hex
        .as_deref()
        .and_then(parse_hex_color)
        .unwrap_or([255, 248, 240]);
    let outline = face.cosmetics.outline_mode;
    let name_scale = name_scale(face.cosmetics.font_family.as_deref());
    let name = fit_text(&fonts.regular, name_scale, &face.character_name, CARD_W - 48);
    draw_outlined(fonts, img, &name, 24, 408, name_scale, fill, outline);

    let series_scale = PxScale::from(20.0);
    let series = fit_text(
        &fonts.regular,
        series_scale,
        &format!("{}  {}", face.series_name, face.series_id),
        CARD_W - 48,
    );
    draw_outlined(
        fonts,
        img,
        &series,
        24,
        448,
        series_scale,
        [220, 214, 235],
        outline,
    );

    let rarity = RarityTier::parse(&face.rarity);
    let mut meta = rarity.as_str().to_string();
    if face.variant != "normal" && !face.variant.is_empty() {
        meta.push_str("  ");
        meta.push_str(&face.variant);
    }
    if let Some(print) = face.print_number {
        meta.push_str(&format!("  #{print}"));
    }
    if let Some(uid) = &face.card_uid {
        meta.push_str("  ");
        meta.push_str(uid);
    }
    draw_outlined(
        fonts,
        img,
        &fit_text(&fonts.regular, PxScale::from(18.0), &meta, CARD_W - 48),
        24,
        478,
        PxScale::from(18.0),
        rarity_color(rarity),
        OutlineMode::Black,
    );

    if let Some(slot) = face.slot_label {
        draw_outlined(
            fonts,
            img,
            &slot.to_string(),
            22,
            22,
            PxScale::from(36.0),
            [255, 255, 255],
            OutlineMode::Black,
        );
    }
}

fn draw_outlined(
    fonts: &Fonts,
    img: &mut RgbaImage,
    text: &str,
    x: i32,
    y: i32,
    scale: PxScale,
    fill: [u8; 3],
    outline: OutlineMode,
) {
    let outline_color = match outline {
        OutlineMode::None => None,
        OutlineMode::Black => Some(Rgba([0, 0, 0, 255])),
        OutlineMode::White => Some(Rgba([255, 255, 255, 255])),
    };
    if let Some(color) = outline_color {
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                draw_text_mut(img, color, x + dx, y + dy, scale, &fonts.regular, text);
            }
        }
    }
    draw_text_mut(
        img,
        Rgba([fill[0], fill[1], fill[2], 255]),
        x,
        y,
        scale,
        &fonts.regular,
        text,
    );
}

fn fit_text(font: &FontArc, scale: PxScale, text: &str, max_w: u32) -> String {
    if text.is_empty() {
        return String::new();
    }
    if text_size(scale, font, text).0 <= max_w {
        return text.to_string();
    }
    let mut kept: String = text.chars().take(48).collect();
    while kept.chars().count() > 1 && text_size(scale, font, &format!("{kept}...")).0 > max_w {
        kept.pop();
    }
    format!("{kept}...")
}

fn name_scale(font_family: Option<&str>) -> PxScale {
    let bump = font_family.map(|id| (hash_str(id) % 5) as f32).unwrap_or(0.0);
    PxScale::from(32.0 + bump)
}

fn cover(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (sw, sh) = src.dimensions();
    if sw == 0 || sh == 0 {
        return RgbaImage::from_pixel(w, h, Rgba([20, 16, 28, 255]));
    }
    let scale = (w as f32 / sw as f32).max(h as f32 / sh as f32);
    let nw = ((sw as f32) * scale).ceil().max(1.0) as u32;
    let nh = ((sh as f32) * scale).ceil().max(1.0) as u32;
    let resized = image::imageops::resize(src, nw, nh, image::imageops::FilterType::Triangle);
    let x = nw.saturating_sub(w) / 2;
    let y = nh.saturating_sub(h) / 2;
    image::imageops::crop_imm(&resized, x, y, w, h).to_image()
}

fn blit(dest: &mut RgbaImage, src: &RgbaImage, ox: u32, oy: u32) {
    for (x, y, pixel) in src.enumerate_pixels() {
        let dx = ox + x;
        let dy = oy + y;
        if dx < dest.width() && dy < dest.height() {
            dest.put_pixel(dx, dy, *pixel);
        }
    }
}

fn dim(img: &mut RgbaImage) {
    for pixel in img.pixels_mut() {
        pixel.0[0] = pixel.0[0] / 3;
        pixel.0[1] = pixel.0[1] / 3;
        pixel.0[2] = pixel.0[2] / 3;
    }
}

fn hollow_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, t: u32, color: Rgba<u8>) {
    fill_rect(img, x as i32, y as i32, w as i32, t as i32, color);
    fill_rect(img, x as i32, (y + h - t) as i32, w as i32, t as i32, color);
    fill_rect(img, x as i32, y as i32, t as i32, h as i32, color);
    fill_rect(img, (x + w - t) as i32, y as i32, t as i32, h as i32, color);
}

fn fill_rect(img: &mut RgbaImage, x0: i32, y0: i32, w: i32, h: i32, color: Rgba<u8>) {
    if w <= 0 || h <= 0 {
        return;
    }
    for y in y0..(y0 + h) {
        for x in x0..(x0 + w) {
            if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
                blend_pixel(img, x as u32, y as u32, color);
            }
        }
    }
}

fn blend_pixel(img: &mut RgbaImage, x: u32, y: u32, src: Rgba<u8>) {
    if src.0[3] == 255 {
        img.put_pixel(x, y, src);
        return;
    }
    let dst = img.get_pixel(x, y).0;
    let a = src.0[3] as u32;
    let inv = 255 - a;
    let mix = |s: u8, d: u8| ((s as u32 * a + d as u32 * inv) / 255) as u8;
    img.put_pixel(
        x,
        y,
        Rgba([
            mix(src.0[0], dst[0]),
            mix(src.0[1], dst[1]),
            mix(src.0[2], dst[2]),
            255,
        ]),
    );
}

fn rarity_color(rarity: RarityTier) -> [u8; 3] {
    match rarity {
        RarityTier::Standard => [210, 210, 220],
        RarityTier::Rare => [88, 166, 255],
        RarityTier::Shiny => [255, 196, 70],
        RarityTier::ThreeD => [196, 120, 255],
        RarityTier::Event => [255, 92, 138],
    }
}

fn palette(seed: u32) -> [u8; 3] {
    [
        40 + (seed & 0xff) as u8 / 2,
        30 + ((seed >> 8) & 0xff) as u8 / 2,
        60 + ((seed >> 16) & 0xff) as u8 / 2,
    ]
}

fn hash_str(value: &str) -> u32 {
    let mut hash = 2166136261u32;
    for byte in value.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(16777619);
    }
    hash
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

fn sat(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, BotError> {
    let mut bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    use image::ImageEncoder;
    encoder
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(BotError::from)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> CardFace {
        CardFace {
            character_name: name.into(),
            series_name: "Naruto".into(),
            series_id: "086c62".into(),
            rarity: "shiny".into(),
            variant: "shiny".into(),
            print_number: Some(7),
            card_uid: Some("Ab12Cd34".into()),
            base_png: None,
            cosmetics: Cosmetics {
                frame_id: Some("frame_1".into()),
                filter_id: Some("filter_holo".into()),
                effect_3d_id: Some("effect_3d_1".into()),
                font_family: Some("font_1".into()),
                text_color_hex: Some("#ffe28a".into()),
                outline_mode: OutlineMode::Black,
            },
            slot_label: Some(1),
            claimed: false,
        }
    }

    #[test]
    fn card_and_strip_encode_to_png() {
        let fonts = Fonts::load().unwrap();
        let one = render_card(&fonts, &sample("Naruto Uzumaki")).unwrap();
        assert!(one.starts_with(b"\x89PNG"));
        let faces = [sample("A"), sample("B"), sample("C")];
        let strip = render_drop_strip(&fonts, &faces).unwrap();
        assert!(strip.len() > one.len());
    }
}
