use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::content::{ArtefactType, Content, ItemId, UnitId};
use razdor::rules::items::heal_amount;
use razdor::rules::units::Stats;

use super::dt_art::DtArt;

/// Every unit and item picture goes through here. Defaults to coloured tokens; PNGs named
/// `<key>.png` (the unit's or item's `Key=`) in the `RAZDOR_ASSETS` directory override them.
/// With a Discord Times install (`RAZDOR_DT_DIR`), the original portraits and item icons are
/// drawn instead of tokens.
pub struct Assets {
    content: Arc<Content>,
    sprites: HashMap<UnitId, Texture2D>,
    item_sprites: HashMap<ItemId, Texture2D>,
    /// Original art from the player's install, if present.
    pub dt: Option<DtArt>,
}

/// Discord Times unit (`GlobalIndex`) whose portrait stands in for a demo unit, by its `Key=`.
fn dt_stand_in(key: &str) -> Option<u32> {
    Some(match key {
        "knight" => 1,
        "archmage" => 2,
        "ranger" => 3,
        "spearman" => 6,
        "archer" => 21,
        "swordsman" => 12,
        "healer" => 26,
        "bandit" => 62,
        "bandit_archer" => 80,
        "bandit_chief" => 89,
        _ => return None,
    })
}

fn item_token(content: &Content, item: ItemId) -> (Color, &'static str) {
    use ArtefactType::*;
    let def = content.item(item);
    match def.kind {
        BlowWeapon => (Color::from_rgba(190, 190, 200, 255), "W"),
        ShotWeapon => (Color::from_rgba(150, 190, 120, 255), "B"),
        Staff => (Color::from_rgba(160, 120, 220, 255), "T"),
        Armor => (Color::from_rgba(140, 140, 150, 255), "A"),
        Helm => (Color::from_rgba(160, 130, 90, 255), "H"),
        Shield => (Color::from_rgba(150, 100, 60, 255), "S"),
        Ring => (Color::from_rgba(230, 200, 80, 255), "R"),
        Amulet => (Color::from_rgba(90, 200, 190, 255), "M"),
        Item => (Color::from_rgba(200, 200, 170, 255), "$"),
        Potion if heal_amount(def) > 0 => (Color::from_rgba(220, 70, 70, 255), "P"),
        Potion => (Color::from_rgba(200, 120, 230, 255), "P"),
    }
}

async fn load_png(path: &str) -> Option<Texture2D> {
    if !Path::new(path).exists() {
        return None;
    }
    match load_texture(path).await {
        Ok(tex) => {
            tex.set_filter(FilterMode::Nearest);
            Some(tex)
        }
        Err(e) => {
            eprintln!("could not load {path}: {e}");
            None
        }
    }
}

/// Placeholder token: colour by role, the first letter of the name.
fn token(content: &Content, kind: UnitId) -> (Color, String) {
    let s = Stats::of_level(content, kind, 1);
    let fill = if s.is_mage() {
        Color::from_rgba(150, 120, 220, 255)
    } else if s.is_shooter() {
        Color::from_rgba(110, 180, 120, 255)
    } else {
        Color::from_rgba(185, 170, 150, 255)
    };
    let letter = content.unit(kind).name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    (fill, letter)
}

pub fn team_color(team: Team) -> Color {
    match team {
        Team::Player => Color::from_rgba(70, 130, 230, 255),
        Team::Enemy => Color::from_rgba(220, 60, 50, 255),
    }
}

impl Assets {
    /// Original portrait: a demo unit uses its stand-in, original content its own `GlobalIndex`.
    fn dt_portrait(&self, kind: UnitId) -> Option<Texture2D> {
        let dt = self.dt.as_ref()?;
        let id = match self.content.unit(kind).extra.get("Key") {
            Some(key) => dt_stand_in(key)?,
            None => kind.0,
        };
        dt.unit_portrait(id)
    }

    /// Original item icon, for original content only (demo items have their own keys).
    fn dt_item_icon(&self, item: ItemId) -> Option<Texture2D> {
        if self.content.item(item).extra.contains_key("Key") {
            return None;
        }
        self.dt.as_ref()?.item_icon(item.0)
    }

    pub async fn load(content: Arc<Content>) -> Self {
        let mut sprites = HashMap::new();
        let mut item_sprites = HashMap::new();
        if let Ok(dir) = std::env::var("RAZDOR_ASSETS") {
            for kind in content.unit_ids() {
                if let Some(tex) = load_png(&format!("{dir}/{}.png", content.unit_key(kind))).await {
                    sprites.insert(kind, tex);
                }
            }
            for item in content.item_ids() {
                if let Some(tex) = load_png(&format!("{dir}/{}.png", content.item_key(item))).await {
                    item_sprites.insert(item, tex);
                }
            }
        }
        Assets { content, sprites, item_sprites, dt: DtArt::from_env() }
    }

    /// Draw an item icon filling the square at (x, y).
    pub fn draw_item(&self, item: ItemId, x: f32, y: f32, size: f32) {
        if let Some(tex) = self.item_sprites.get(&item).cloned().or_else(|| self.dt_item_icon(item)) {
            let params = DrawTextureParams { dest_size: Some(vec2(size, size)), ..Default::default() };
            draw_texture_ex(&tex, x, y, WHITE, params);
            return;
        }
        let (fill, letter) = item_token(&self.content, item);
        let pad = size * 0.12;
        draw_rectangle(x + pad, y + pad, size - 2.0 * pad, size - 2.0 * pad, fill);
        draw_rectangle_lines(x + pad, y + pad, size - 2.0 * pad, size - 2.0 * pad, 2.0, BLACK);
        let fs = (size * 0.5) as u16;
        let dim = measure_text(letter, None, fs, 1.0);
        draw_text(letter, x + (size - dim.width) / 2.0, y + (size + dim.offset_y) / 2.0, fs as f32, BLACK);
    }

    /// Draw a unit centred on (cx, cy) inside a square of `size`.
    pub fn draw_unit(&self, kind: UnitId, team: Team, cx: f32, cy: f32, size: f32) {
        let ring = team_color(team);
        let dt_portrait = || self.dt_portrait(kind);
        if let Some(tex) = self.sprites.get(&kind).cloned().or_else(dt_portrait) {
            draw_circle(cx, cy + size * 0.38, size * 0.4, Color { a: 0.5, ..ring });
            draw_texture_ex(
                &tex,
                cx - size / 2.0,
                cy - size / 2.0,
                WHITE,
                DrawTextureParams { dest_size: Some(vec2(size, size)), ..Default::default() },
            );
            return;
        }
        let (fill, letter) = token(&self.content, kind);
        let r = size * 0.38;
        draw_circle(cx, cy, r + 3.0, ring);
        draw_circle(cx, cy, r, fill);
        let fs = (size * 0.5) as u16;
        let dim = measure_text(&letter, None, fs, 1.0);
        draw_text(&letter, cx - dim.width / 2.0, cy + dim.offset_y / 2.0, fs as f32, BLACK);
    }
}
