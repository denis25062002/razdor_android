//! The interface language: the player's choice kept in `settings.json` (next to
//! `audio.json` in the save folder), the EN / RU switch and its F2 key. Russian when nothing
//! is saved (English when no font with Cyrillic was found). The same file remembers that
//! the tutorial is done.

use std::path::PathBuf;

use macroquad::prelude::*;
use razdor::i18n::{self, tr, Lang};
use serde::{Deserialize, Serialize};

use super::widgets::*;

/// The key that switches the language on every screen.
pub const KEY: KeyCode = KeyCode::F2;

/// Settings of the game that are not about sound, saved in `settings.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "en" or "ru"; empty: never chosen.
    pub language: String,
    /// The tutorial's last event finished (its title test holds `end_tutorial`): the original
    /// writes `[Tutorial] Completed=1` into its language ini (0x4ac9fc); Razdor keeps it here.
    pub tutorial_completed: bool,
}

impl Settings {
    fn path() -> Option<PathBuf> {
        razdor::rules::save::default_dir().map(|d| d.join("settings.json"))
    }

    pub fn load() -> Settings {
        let read = Settings::path().and_then(|p| std::fs::read(p).ok());
        read.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Writes the settings, keeping fields of the file this version does not know.
    pub fn save(&self) {
        let Some(path) = Settings::path() else { return };
        let mut json = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        json["language"] = serde_json::Value::String(self.language.clone());
        json["tutorial_completed"] = serde_json::Value::Bool(self.tutorial_completed);
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| {
            std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap_or_default())
        });
        if let Err(e) = written {
            razdor::diag!("{}: {e}", path.display());
        }
    }
}

/// The language to start in: the saved one, else Russian (English without a Cyrillic font,
/// which would only show transliterations).
pub fn initial(saved: &Settings, cyrillic_font: bool) -> Lang {
    Lang::from_code(&saved.language).unwrap_or(if cyrillic_font { Lang::Ru } else { Lang::En })
}

/// At startup: sets the saved language.
pub fn init() {
    i18n::set_lang(initial(&Settings::load(), has_font()));
}

/// Switches to the other language and saves the choice.
pub fn toggle() {
    let next = i18n::lang().other();
    i18n::set_lang(next);
    Settings { language: next.code().to_string(), ..Settings::load() }.save();
}

/// The EN / RU switch: both codes, the current one lit; a click switches. True when it did.
pub fn switch_button(x: f32, y: f32, w: f32, h: f32) -> bool {
    let hover = mouse_in(x, y, w, h);
    draw_rectangle(x, y, w, h, PANEL);
    draw_rectangle_lines(x, y, w, h, 2.0, if hover { ACCENT } else { DIM });
    let size = (h * 0.55).clamp(12.0, 22.0);
    let cur = i18n::lang();
    let (en, slash, ru) = (Lang::En.label(), " / ", Lang::Ru.label());
    let total = measure(en, size).width + measure(slash, size).width + measure(ru, size).width;
    let mut tx = x + (w - total) / 2.0;
    let ty = y + h / 2.0 + size * 0.35;
    for (s, lit) in [(en, cur == Lang::En), (slash, false), (ru, cur == Lang::Ru)] {
        text(s, tx, ty, size, if lit { ACCENT } else { DIM });
        tx += measure(s, size).width;
    }
    if hover {
        tooltip(&[(tr("Interface language (F2)").to_string(), INK)]);
    }
    if hover && clicked() {
        super::audio::cue(super::audio::Cue::Button);
        toggle();
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_unless_chosen_otherwise() {
        assert_eq!(initial(&Settings::default(), true), Lang::Ru, "the default");
        assert_eq!(initial(&Settings::default(), false), Lang::En, "no Cyrillic font");
        assert_eq!(initial(&Settings { language: "en".into(), ..Settings::default() }, true), Lang::En);
        assert_eq!(initial(&Settings { language: "ru".into(), ..Settings::default() }, false), Lang::Ru, "a choice is kept");
        assert_eq!(initial(&Settings { language: "xx".into(), ..Settings::default() }, true), Lang::Ru);
        let s: Settings = serde_json::from_str(r#"{"language":"en","other":1,"tutorial_offered":true}"#).unwrap();
        assert_eq!(s.language, "en");
        assert!(!s.tutorial_completed, "older files: the tutorial is not done (an offer seen does not count)");
    }
}
