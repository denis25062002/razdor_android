use std::collections::HashMap;
use std::path::Path;

use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::items::{catalog, Effect, ItemId, ItemType};
use razdor::rules::units::UnitKind;

use super::dt_art::DtArt;

/// Every unit and item picture goes through here. Defaults to coloured tokens; PNGs named
/// `<asset_key>.png` (units) or `<item id>.png` (items) in the `RAZDOR_ASSETS` directory
/// override them. With a Discord Times install (`RAZDOR_DT_DIR`), the original portraits are
/// drawn instead of tokens.
pub struct Assets {
    sprites: HashMap<UnitKind, Texture2D>,
    item_sprites: HashMap<ItemId, Texture2D>,
    /// Original art from the player's install, if present.
    pub dt: Option<DtArt>,
}

/// Discord Times unit (`GlobalIndex`) whose portrait stands in for a demo unit.
fn dt_stand_in(kind: UnitKind) -> u32 {
    use UnitKind::*;
    match kind {
        Knight => 1,
        Archmage => 2,
        Ranger => 3,
        Spearman => 6,
        Archer => 21,
        Swordsman => 12,
        Healer => 26,
        Bandit => 62,
        BanditArcher => 80,
        BanditChief => 89,
    }
}

fn item_token(item: ItemId) -> (Color, &'static str) {
    use ItemType::*;
    let def = item.def();
    match def.ty {
        Weapon => (Color::from_rgba(190, 190, 200, 255), "W"),
        Armor => (Color::from_rgba(140, 140, 150, 255), "A"),
        Helmet => (Color::from_rgba(160, 130, 90, 255), "H"),
        Shield => (Color::from_rgba(150, 100, 60, 255), "S"),
        Ring => (Color::from_rgba(230, 200, 80, 255), "R"),
        Amulet => (Color::from_rgba(90, 200, 190, 255), "M"),
        Boots => (Color::from_rgba(120, 90, 60, 255), "B"),
        Cloak => (Color::from_rgba(90, 110, 170, 255), "C"),
        Potion => match def.effect {
            Some(Effect::Heal(_)) => (Color::from_rgba(220, 70, 70, 255), "P"),
            _ => (Color::from_rgba(200, 120, 230, 255), "P"),
        },
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

fn token(kind: UnitKind) -> (Color, &'static str) {
    use UnitKind::*;
    match kind {
        Knight => (Color::from_rgba(200, 200, 215, 255), "K"),
        Archmage => (Color::from_rgba(140, 110, 220, 255), "M"),
        Ranger => (Color::from_rgba(90, 170, 90, 255), "R"),
        Spearman => (Color::from_rgba(180, 150, 100, 255), "S"),
        Archer => (Color::from_rgba(120, 180, 140, 255), "A"),
        Swordsman => (Color::from_rgba(170, 170, 180, 255), "W"),
        Healer => (Color::from_rgba(240, 230, 180, 255), "H"),
        Bandit => (Color::from_rgba(150, 90, 70, 255), "B"),
        BanditArcher => (Color::from_rgba(170, 120, 80, 255), "b"),
        BanditChief => (Color::from_rgba(120, 50, 40, 255), "C"),
    }
}

pub fn team_color(team: Team) -> Color {
    match team {
        Team::Player => Color::from_rgba(70, 130, 230, 255),
        Team::Enemy => Color::from_rgba(220, 60, 50, 255),
    }
}

impl Assets {
    pub async fn load() -> Self {
        let mut sprites = HashMap::new();
        let mut item_sprites = HashMap::new();
        if let Ok(dir) = std::env::var("RAZDOR_ASSETS") {
            for kind in UnitKind::ALL {
                if let Some(tex) = load_png(&format!("{dir}/{}.png", kind.asset_key())).await {
                    sprites.insert(kind, tex);
                }
            }
            for item in catalog().ids() {
                if let Some(tex) = load_png(&format!("{dir}/{}.png", item.def().id)).await {
                    item_sprites.insert(item, tex);
                }
            }
        }
        Assets { sprites, item_sprites, dt: DtArt::from_env() }
    }

    /// Draw an item icon filling the square at (x, y).
    pub fn draw_item(&self, item: ItemId, x: f32, y: f32, size: f32) {
        if let Some(tex) = self.item_sprites.get(&item) {
            let params = DrawTextureParams { dest_size: Some(vec2(size, size)), ..Default::default() };
            draw_texture_ex(tex, x, y, WHITE, params);
            return;
        }
        let (fill, letter) = item_token(item);
        let pad = size * 0.12;
        draw_rectangle(x + pad, y + pad, size - 2.0 * pad, size - 2.0 * pad, fill);
        draw_rectangle_lines(x + pad, y + pad, size - 2.0 * pad, size - 2.0 * pad, 2.0, BLACK);
        let fs = (size * 0.5) as u16;
        let dim = measure_text(letter, None, fs, 1.0);
        draw_text(letter, x + (size - dim.width) / 2.0, y + (size + dim.offset_y) / 2.0, fs as f32, BLACK);
    }

    /// Draw a unit centred on (cx, cy) inside a square of `size`.
    pub fn draw_unit(&self, kind: UnitKind, team: Team, cx: f32, cy: f32, size: f32) {
        let ring = team_color(team);
        let dt_portrait = || self.dt.as_ref()?.unit_portrait(dt_stand_in(kind));
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
        let (fill, letter) = token(kind);
        let r = size * 0.38;
        draw_circle(cx, cy, r + 3.0, ring);
        draw_circle(cx, cy, r, fill);
        let fs = (size * 0.5) as u16;
        let dim = measure_text(letter, None, fs, 1.0);
        draw_text(letter, cx - dim.width / 2.0, cy + dim.offset_y / 2.0, fs as f32, BLACK);
    }
}
