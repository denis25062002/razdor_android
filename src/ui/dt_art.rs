//! Original Discord Times art, decoded at runtime from the player's install (`RAZDOR_DT_DIR`)
//! and turned into textures on first use. Nothing is written to disk.

// Part of the API is for the building screens of the next stages.
#![allow(dead_code)]

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use macroquad::prelude::*;

use razdor::dt::gfx::{self, Image, ObjectSprite, ObjectSprites};
use razdor::dt::install::DtInstall;
use razdor::dt::DtError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Portrait(u32),
    Figure(u32),
    Item(u32),
    Terrain(u8),
    Object(u8, u8),
    Building(u8, u8),
}

/// Every map object and building sprite in one texture, so the world map draws in few batches.
pub struct Atlas {
    pub texture: Texture2D,
    /// (section, category, index) → source rectangle in `texture`.
    rects: HashMap<(u32, u32, u32), Rect>,
}

impl Atlas {
    /// Sprite of a `.DTm` map object (class, sprite id).
    pub fn decoration(&self, class: u8, sprite: u8) -> Option<Rect> {
        self.rects.get(&(ObjectSprite::DECORATIONS, class.into(), sprite.into())).copied()
    }

    /// Sprite of a building by picture type and variant.
    pub fn building(&self, picture_type: u8, variant: u8) -> Option<Rect> {
        self.rects.get(&(ObjectSprite::BUILDINGS, picture_type.into(), variant.into())).copied()
    }
}

/// Width of the map-object atlas in pixels.
const ATLAS_WIDTH: u32 = 2048;

/// Lazily decoded sheets and a texture cache. Missing or broken art gives `None`
/// (logged once), so callers fall back to placeholders.
pub struct DtArt {
    pub install: DtInstall,
    portraits: OnceCell<Vec<Image>>,
    figures: OnceCell<Vec<Image>>,
    items: OnceCell<Vec<Image>>,
    objects: OnceCell<ObjectSprites>,
    atlas: OnceCell<Option<Atlas>>,
    textures: RefCell<HashMap<Key, Option<Texture2D>>>,
    /// Map figures (`Graphics/Units/*.ugs`) as 8×8 sheets of 64×64 frames, by file stem.
    figures_sheets: RefCell<HashMap<String, Option<Texture2D>>>,
}

fn or_log<T: Default>(what: &str, r: Result<T, DtError>) -> T {
    r.unwrap_or_else(|e| {
        eprintln!("Discord Times art: {what}: {e}");
        T::default()
    })
}

fn texture(img: &Image) -> Option<Texture2D> {
    let (w, h) = (u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?);
    let tex = Texture2D::from_rgba8(w, h, &img.rgba);
    tex.set_filter(FilterMode::Linear);
    Some(tex)
}

impl DtArt {
    pub fn load(install: DtInstall) -> DtArt {
        DtArt {
            install,
            portraits: OnceCell::new(),
            figures: OnceCell::new(),
            items: OnceCell::new(),
            objects: OnceCell::new(),
            atlas: OnceCell::new(),
            textures: RefCell::new(HashMap::new()),
            figures_sheets: RefCell::new(HashMap::new()),
        }
    }

    /// The player's install ([`DtInstall::from_env`]: `RAZDOR_DT_DIR`, the remembered folder,
    /// or one found in the usual places), if there is one and it loads.
    pub fn from_env() -> Option<DtArt> {
        match DtInstall::from_env() {
            Ok(install) => Some(DtArt::load(install)),
            Err(e) => {
                eprintln!("Discord Times install not usable, using placeholders: {e}");
                None
            }
        }
    }

    fn cached(&self, key: Key, make: impl FnOnce() -> Option<Texture2D>) -> Option<Texture2D> {
        if let Some(t) = self.textures.borrow().get(&key) {
            return t.clone();
        }
        let t = make();
        self.textures.borrow_mut().insert(key, t.clone());
        t
    }

    /// Colour bust (92×92) of a unit, by `GlobalIndex`.
    pub fn unit_portrait(&self, unit_id: u32) -> Option<Texture2D> {
        self.cached(Key::Portrait(unit_id), || {
            let sheet = self.portraits.get_or_init(|| or_log("unit portraits", self.install.unit_portraits()));
            texture(sheet.get(gfx::portrait_frame(unit_id)?)?)
        })
    }

    /// Full-body sepia portrait of a unit, by `GlobalIndex`.
    pub fn unit_figure(&self, unit_id: u32) -> Option<Texture2D> {
        self.cached(Key::Figure(unit_id), || {
            let sheet = self.figures.get_or_init(|| or_log("unit figures", self.install.unit_figures()));
            texture(sheet.get(gfx::portrait_frame(unit_id)?)?)
        })
    }

    /// Icon (53×53) of an artefact, by `GlobalIndex`.
    pub fn item_icon(&self, artefact_id: u32) -> Option<Texture2D> {
        self.cached(Key::Item(artefact_id), || {
            let sheet = self.items.get_or_init(|| or_log("item icons", self.install.item_icons()));
            texture(sheet.get(self.install.artefact_icon_frame(artefact_id)?)?)
        })
    }

    /// World-map texture of a terrain code (tiles seamlessly).
    pub fn terrain(&self, code: u8) -> Option<Texture2D> {
        self.cached(Key::Terrain(code), || {
            texture(&or_log("terrain texture", self.install.terrain_texture(code).map(Some))?)
        })
    }

    /// The decoded `Objects.ugs` (the editor's palette).
    pub fn objects(&self) -> &ObjectSprites {
        self.objects.get_or_init(|| or_log("map objects", self.install.map_objects()))
    }

    /// Sprite of a map object (`.DTm` object class and sprite id).
    pub fn object(&self, class: u8, sprite: u8) -> Option<Texture2D> {
        self.cached(Key::Object(class, sprite), || texture(&self.objects().decoration(class, sprite)?.image))
    }

    /// All map objects and buildings packed into one texture.
    pub fn map_atlas(&self) -> Option<&Atlas> {
        self.atlas
            .get_or_init(|| {
                let sprites = &self.objects().sprites;
                let images: Vec<&Image> = sprites.iter().map(|s| &s.image).collect();
                let Some((image, pos)) = gfx::pack_atlas(&images, ATLAS_WIDTH) else {
                    eprintln!("Discord Times art: map objects do not fit an atlas");
                    return None;
                };
                let texture = texture(&image)?;
                let rects = sprites
                    .iter()
                    .zip(pos)
                    .map(|(s, (x, y))| ((s.section, s.cat, s.idx), Rect::new(x as f32, y as f32, s.image.width as f32, s.image.height as f32)))
                    .collect();
                Some(Atlas { texture, rects })
            })
            .as_ref()
    }

    /// A map figure sheet (`Graphics/Units/<stem>.ugs`): 8 rows (facings, clockwise from
    /// north-west) of 8 walking frames, 64×64 each.
    pub fn figure_sheet(&self, stem: &str) -> Option<Texture2D> {
        if let Some(t) = self.figures_sheets.borrow().get(stem) {
            return t.clone();
        }
        let frames = or_log("map figures", self.install.graphic(&format!("Graphics/Units/{stem}.ugs")));
        let t = (frames.len() == 64 && frames.iter().all(|f| f.width == 64 && f.height == 64))
            .then(|| {
                let refs: Vec<&Image> = frames.iter().collect();
                let mut rgba = vec![0u8; 512 * 512 * 4];
                for (i, f) in refs.iter().enumerate() {
                    let (ox, oy) = ((i % 8) * 64, (i / 8) * 64);
                    for row in 0..64 {
                        let dst = ((oy + row) * 512 + ox) * 4;
                        rgba[dst..dst + 256].copy_from_slice(&f.rgba[row * 256..row * 256 + 256]);
                    }
                }
                texture(&Image { width: 512, height: 512, rgba })
            })
            .flatten();
        self.figures_sheets.borrow_mut().insert(stem.to_string(), t.clone());
        t
    }

    /// Sprite of a building by picture type and variant (`.DTm` building bytes 5 and 4).
    pub fn building(&self, picture_type: u8, variant: u8) -> Option<Texture2D> {
        self.cached(Key::Building(picture_type, variant), || {
            texture(&self.objects().building(picture_type, variant)?.image)
        })
    }
}
