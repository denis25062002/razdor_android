//! Keys that work across screens (F1 help, F5 quick save, F9 quick load), when they may
//! fire, and the F1 overlay listing every screen's keys (Razdor extras the players asked
//! for). A screen's own keys live in its module; this one only lists them for the overlay.
//!
//! No key fires while the player types (the class screen's hero name, the save name, an
//! editor field) or while a dialog or question is open (there N means "No", Y "Yes").

use macroquad::prelude::*;

use super::chrome::{self, Skin};
use super::widgets::*;

/// Where the keys are pressed: the app's screens, by what their keys do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Title,
    ClassSelect,
    WorldMap,
    Building,
    Army,
    Battle { deploying: bool },
    Journal,
    Spellbook,
    Menu,
    Save,
    Load,
    /// Victory or defeat.
    End,
    Editor,
}

/// A key that works across screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Global {
    /// F1: the key list.
    Help,
    /// F5: the quick save.
    QuickSave,
    /// F9: load the quick save.
    QuickLoad,
    /// F2: the interface language, English / Russian.
    Language,
}

impl Global {
    #[cfg(test)]
    pub const ALL: [Global; 4] = [Global::Help, Global::QuickSave, Global::QuickLoad, Global::Language];

    pub fn key(self) -> KeyCode {
        match self {
            Global::Help => KeyCode::F1,
            Global::QuickSave => KeyCode::F5,
            Global::QuickLoad => KeyCode::F9,
            Global::Language => super::language::KEY,
        }
    }
}

/// What stands in the way of keys this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Guard {
    /// A text field has the keyboard.
    pub typing: bool,
    /// A dialog or a question is open.
    pub dialog: bool,
    /// A game is loaded.
    pub game: bool,
    /// A battle is pending (saving waits until it is over, as in the menu).
    pub foe: bool,
}

/// The screens with a text field that always has the keyboard.
pub fn always_typing(place: Place) -> bool {
    matches!(place, Place::ClassSelect | Place::Save)
}

/// Whether the player is typing: on a screen with a text field, or a field has the focus.
pub fn typing(place: Place, field_focused: bool) -> bool {
    always_typing(place) || field_focused
}

/// Whether single keys may act as shortcuts now (N for the music, letters on the map).
pub fn shortcuts_allowed(g: Guard) -> bool {
    !g.typing && !g.dialog
}

/// Whether `key` may fire at `place`. The editor has its own keys (and F2).
pub fn allowed(place: Place, key: Global, g: Guard) -> bool {
    if !shortcuts_allowed(g) || (place == Place::Editor && key != Global::Language) {
        return false;
    }
    match key {
        Global::Help => true,
        Global::QuickSave => {
            g.game && !g.foe && matches!(place, Place::WorldMap | Place::Building | Place::Army | Place::Journal | Place::Spellbook | Place::Menu)
        }
        Global::QuickLoad | Global::Language => true,
    }
}

/// The keys of `place` for the F1 overlay: (key, what it does).
pub fn screen_keys(place: Place) -> Vec<(&'static str, &'static str)> {
    match place {
        Place::Title => vec![("Click", "pick a scenario"), ("F9", "load the quick save")],
        Place::ClassSelect => vec![("Type", "the hero's name"), ("Backspace", "delete a letter")],
        Place::WorldMap => vec![
            ("Click", "walk there (a building: enter; an army: meet it)"),
            ("Right click / Space", "stop"),
            ("Wheel, + / -", "zoom"),
            ("1 / 4", "wait 1 or 4 hours"),
            ("M", "minimap"),
            ("Tab", "centre the camera on the hero"),
            ("J", "journal"),
            ("B", "spell book"),
            ("A", "hero and army"),
            ("F5 / F9", "quick save / quick load"),
            ("Esc", "close the minimap, else the game menu"),
        ],
        Place::Building => vec![("Click", "tabs and buttons"), ("F5", "quick save"), ("Esc", "back to the map")],
        Place::Army => vec![("Click", "a unit, an item"), ("F5", "quick save"), ("A / Esc", "close")],
        Place::Battle { deploying: true } => vec![
            ("Click a card, then a cell", "move it"),
            ("Fight!", "start the battle"),
            ("Q / Enter", "quick battle: played out at once"),
        ],
        Place::Battle { deploying: false } => vec![
            ("Click a framed card", "attack or cast"),
            ("Click a lit cell", "step there"),
            ("Click your own card", "pass one action"),
            ("Space", "end the unit's turn"),
            ("Q", "finish the battle automatically"),
            ("Enter", "OK on the result"),
        ],
        Place::Journal => vec![
            ("Left / Right", "change the tab"),
            ("Up / Down, click", "pick an entry"),
            ("Wheel, PgUp / PgDn", "scroll"),
            ("F5", "quick save"),
            ("J / Esc", "close"),
        ],
        Place::Spellbook => vec![("Click", "pick a spell"), ("Enter", "cast on your army"), ("F5", "quick save"), ("B / Esc", "close")],
        Place::Menu => vec![("+ / -", "music volume"), ("F5", "quick save"), ("Esc", "back to the game")],
        Place::Save => vec![("Type", "the save's name"), ("Enter", "save"), ("Esc", "cancel")],
        Place::Load => vec![("Click", "pick a save"), ("Enter", "load it"), ("Esc", "cancel")],
        Place::End => vec![("Click", "the buttons"), ("F9", "load the quick save")],
        Place::Editor => vec![],
    }
}

/// Keys that work on every screen of a game, and in the dialogs.
pub const EVERYWHERE: [(&str, &str); 6] = [
    ("F1", "this list (F1 or Esc closes it)"),
    ("F2", "interface language: English / Russian"),
    ("F9", "load the quick save"),
    ("N", "music off / on"),
    ("Y / Enter", "\"Yes\" in a question (Enter: OK)"),
    ("N / Esc", "\"No\" in a question"),
];

/// A screen's title in the overlay.
fn place_name(place: Place) -> &'static str {
    match place {
        Place::Title => "Title screen",
        Place::ClassSelect => "Hero choice",
        Place::WorldMap => "World map",
        Place::Building => "Building",
        Place::Army => "Hero and army",
        Place::Battle { deploying: true } => "Battle: deployment",
        Place::Battle { deploying: false } => "Battle",
        Place::Journal => "Journal",
        Place::Spellbook => "Spell book",
        Place::Menu => "Game menu",
        Place::Save => "Save",
        Place::Load => "Load",
        Place::End => "End of the game",
        Place::Editor => "Map editor",
    }
}

/// Draws the F1 overlay over the screen; true when it closes (F1, Esc or a click).
pub fn help_overlay(place: Place) -> bool {
    let (sw, sh) = (screen_width(), screen_height());
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.45));
    let own = screen_keys(place);
    let lines = own.len() + EVERYWHERE.len() + 3;
    let (w, h) = (640.0f32.min(sw - 20.0), (90.0 + lines as f32 * 24.0).min(sh - 20.0));
    let (x, y) = ((sw - w) / 2.0, (sh - h) / 2.0);
    chrome::window(Rect::new(x, y, w, h), "Keys", Skin::Marble, false);
    let kx = x + 24.0;
    let dx = x + w * 0.42;
    let mut ly = y + 58.0;
    let section = |title: &str, rows: &[(&str, &str)], ly: &mut f32| {
        text(title, kx, *ly, 19.0, ACCENT);
        *ly += 26.0;
        for (k, what) in rows {
            if *ly > y + h - 34.0 {
                break;
            }
            text(k, kx + 10.0, *ly, 17.0, INK);
            text(what, dx, *ly, 17.0, DIM);
            *ly += 24.0;
        }
        *ly += 6.0;
    };
    if !own.is_empty() {
        section(place_name(place), &own, &mut ly);
    }
    section("Everywhere", &EVERYWHERE, &mut ly);
    text_centered("F1, Esc or a click closes this list", x + w / 2.0, y + h - 14.0, 15.0, DIM);
    is_key_pressed(KeyCode::F1) || is_key_pressed(KeyCode::Escape) || is_mouse_button_pressed(MouseButton::Left)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PLACES: [Place; 14] = [
        Place::Title,
        Place::ClassSelect,
        Place::WorldMap,
        Place::Building,
        Place::Army,
        Place::Battle { deploying: true },
        Place::Battle { deploying: false },
        Place::Journal,
        Place::Spellbook,
        Place::Menu,
        Place::Save,
        Place::Load,
        Place::End,
        Place::Editor,
    ];

    fn in_game() -> Guard {
        Guard { game: true, ..Guard::default() }
    }

    #[test]
    fn no_key_fires_while_typing_or_in_a_dialog() {
        for place in ALL_PLACES {
            for key in Global::ALL {
                assert!(!allowed(place, key, Guard { typing: true, ..in_game() }), "{place:?} {key:?} while typing");
                assert!(!allowed(place, key, Guard { dialog: true, ..in_game() }), "{place:?} {key:?} in a dialog");
            }
        }
        assert!(!shortcuts_allowed(Guard { dialog: true, ..in_game() }), "N is No in a question");
        assert!(typing(Place::Save, false) && typing(Place::ClassSelect, false) && typing(Place::WorldMap, true));
        assert!(!typing(Place::WorldMap, false));
    }

    #[test]
    fn quick_save_only_in_a_game_between_battles() {
        assert!(allowed(Place::WorldMap, Global::QuickSave, in_game()));
        assert!(allowed(Place::Building, Global::QuickSave, in_game()));
        assert!(!allowed(Place::WorldMap, Global::QuickSave, Guard { foe: true, ..in_game() }), "a battle is pending");
        assert!(!allowed(Place::Battle { deploying: true }, Global::QuickSave, in_game()));
        assert!(!allowed(Place::Title, Global::QuickSave, Guard::default()));
        assert!(!allowed(Place::End, Global::QuickSave, in_game()));
    }

    #[test]
    fn quick_load_and_help_work_everywhere_but_the_editor() {
        for place in ALL_PLACES.into_iter().filter(|p| *p != Place::Editor && !always_typing(*p)) {
            assert!(allowed(place, Global::QuickLoad, in_game()), "{place:?}");
            assert!(allowed(place, Global::Help, in_game()), "{place:?}");
        }
        assert!(!allowed(Place::Editor, Global::Help, in_game()));
        assert!(allowed(Place::Editor, Global::Language, in_game()), "F2 works in the editor too");
        assert!(!allowed(Place::Editor, Global::Language, Guard { typing: true, ..in_game() }));
    }

    #[test]
    fn every_screen_lists_its_keys() {
        for place in ALL_PLACES.into_iter().filter(|p| *p != Place::Editor) {
            assert!(!screen_keys(place).is_empty(), "{place:?}");
        }
        let map: Vec<&str> = screen_keys(Place::WorldMap).iter().map(|(k, _)| *k).collect();
        for k in ["M", "Tab", "J", "B", "A", "1 / 4", "F5 / F9", "Esc"] {
            assert!(map.contains(&k), "the map lists {k}");
        }
        assert!(screen_keys(Place::Battle { deploying: true }).iter().any(|(k, _)| *k == "Q / Enter"));
    }
}
