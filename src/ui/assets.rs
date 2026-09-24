use std::collections::HashMap;
use std::path::Path;

use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::units::UnitKind;

/// Every unit picture goes through here. Defaults to coloured tokens; PNGs named
/// `<asset_key>.png` in the `RAZDOR_ASSETS` directory override them.
pub struct Assets {
    sprites: HashMap<UnitKind, Texture2D>,
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
        if let Ok(dir) = std::env::var("RAZDOR_ASSETS") {
            for kind in UnitKind::ALL {
                let path = format!("{dir}/{}.png", kind.asset_key());
                if Path::new(&path).exists() {
                    match load_texture(&path).await {
                        Ok(tex) => {
                            tex.set_filter(FilterMode::Nearest);
                            sprites.insert(kind, tex);
                        }
                        Err(e) => eprintln!("could not load {path}: {e}"),
                    }
                }
            }
        }
        Assets { sprites }
    }

    /// Draw a unit centred on (cx, cy) inside a square of `size`.
    pub fn draw_unit(&self, kind: UnitKind, team: Team, cx: f32, cy: f32, size: f32) {
        let ring = team_color(team);
        if let Some(tex) = self.sprites.get(&kind) {
            draw_circle(cx, cy + size * 0.38, size * 0.4, Color { a: 0.5, ..ring });
            draw_texture_ex(
                tex,
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
