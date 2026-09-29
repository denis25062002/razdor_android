//! macroquad presentation layer. Reads rules state and calls rules methods.
pub mod assets;
pub mod audio;
pub mod battle_view;
pub mod building_view;
pub mod chrome;
pub mod dialog;
pub mod dt_art;
pub mod dt_font;
pub mod editor;
pub mod game_bar;
pub mod hotkeys;
pub mod items_view;
pub mod jukebox;
pub mod language;
pub mod main_menu;
pub mod minimap;
pub mod new_game;
pub mod saves;
pub mod screens;
pub mod snapshot;
pub mod spellbook;
pub mod story;
pub mod terrain;
pub mod unit_sheet;
pub mod widgets;
pub mod world_view;

use std::collections::VecDeque;

use macroquad::prelude::{is_key_pressed, KeyCode};
use std::path::PathBuf;
use std::sync::Arc;

use razdor::dt::dtm::Scenario;
use razdor::i18n::tr;
use razdor::rules::content::Content;
use razdor::rules::events::EventOutcome;
use razdor::rules::game::Game;
use razdor::rules::save::{self, Install};
use razdor::rules::script::ScriptEnd;

use assets::Assets;
use audio::{Audio, Cue, Mood};
use battle_view::BattleView;
use building_view::BuildingView;
use dialog::{Close, Dialog};
use world_view::MapView;

pub enum Screen {
    /// The original's main menu (the first screen).
    MainMenu,
    /// The authors' window over the main menu, opened at this time (`get_time`).
    Authors(f64),
    /// The settings window over the main menu.
    Options,
    /// The built-in demo or a map of the install.
    ScenarioSelect,
    /// Hero class for the demo (`None`) or for `scenarios[i]`.
    ClassSelect { scenario: Option<usize> },
    WorldMap,
    /// The window of the building the hero stands in.
    Building(BuildingView),
    /// Hero and army screen: selected squad member, backpack scroll, and the building window
    /// "Back" returns to (the map if none).
    Squad { selected: usize, scroll: usize, back: Option<BuildingView> },
    Battle(Box<BattleView>),
    /// The journal: its tab, selected line and scrolling.
    Journal(story::JournalView),
    /// The spell book, with the selected cell.
    Spellbook { selected: usize },
    /// "Выход из игры" over the map (the bar's X, Esc); `true` while the restart question
    /// is open.
    Menu(bool),
    /// The settings window over the map (the bar's gears).
    Settings,
    Save(saves::SaveView),
    Load(saves::LoadView),
    GameOver,
    Victory,
    /// The map editor (`ui::editor`).
    Editor,
}

/// A scenario of the player's install, loaded for the select screen.
pub struct ScenarioEntry {
    /// File name without the extension.
    pub file: String,
    pub path: PathBuf,
    pub scenario: Scenario,
}

pub struct App {
    pub assets: Assets,
    /// The built-in demo content.
    pub demo: Arc<Content>,
    /// Content of the player's install, if present.
    pub dt_content: Option<Arc<Content>>,
    /// Maps of the install (read at start, never written).
    pub scenarios: Vec<ScenarioEntry>,
    pub game: Option<Game>,
    pub screen: Screen,
    /// One-line notice shown on the world map / town screens.
    pub message: Option<String>,
    /// Modal windows waiting to be read (noon reports, victories), first on top.
    pub dialogs: VecDeque<Dialog>,
    pub map_view: MapView,
    /// A save file the load window picked: loaded after the frame.
    pub pending_load: Option<PathBuf>,
    /// Why the last load failed (shown in the load window).
    pub load_error: Option<String>,
    pub audio: Audio,
    /// The screen of the last frame, to hear windows open and battles begin.
    last_screen: Option<std::mem::Discriminant<Screen>>,
    /// Gold at the end of the last frame of this game (`None` right after a new game or load).
    last_gold: Option<i32>,
    /// The map editor, kept while a test play runs.
    editor: Option<Box<editor::EditorScreen>>,
    /// The game is a test play of the editor's map: leaving it returns to the editor.
    test_play: bool,
    /// The F1 key list is open over the screen.
    help: bool,
    /// The interface language the demo content was built in.
    lang: razdor::i18n::Lang,
    /// "Выход" was picked in the main menu: the process ends after this frame.
    pub quit: bool,
}

impl App {
    pub fn new(assets: Assets, demo: Arc<Content>) -> Self {
        let dt_content = assets.dt.as_ref().map(|d| Arc::new(Content::from_dt(&d.install)));
        let scenarios = assets
            .dt
            .iter()
            .flat_map(|d| d.install.maps.iter())
            .filter_map(|m| match m.load() {
                Ok(scenario) => Some(ScenarioEntry { file: m.name.clone(), path: m.path.clone(), scenario }),
                Err(e) => {
                    eprintln!("{}: {e}", m.name);
                    None
                }
            })
            .collect();
        let audio = Audio::new(assets.dt.as_ref().map(|d| &d.install));
        App {
            assets,
            demo,
            dt_content,
            scenarios,
            game: None,
            screen: Screen::MainMenu,
            message: None,
            dialogs: VecDeque::new(),
            map_view: MapView::default(),
            pending_load: None,
            load_error: None,
            audio,
            last_screen: None,
            last_gold: None,
            editor: None,
            test_play: false,
            help: false,
            lang: razdor::i18n::lang(),
            quit: false,
        }
    }

    /// Opens the map editor (the title screen's button and `--editor`).
    pub fn open_editor(&mut self) {
        if self.editor.is_none() {
            self.editor = Some(Box::new(editor::EditorScreen::new(&self.assets, self.dt_content.clone(), self.demo.clone())));
        }
        self.screen = Screen::Editor;
    }

    /// After an EN / RU switch: the demo's names and descriptions in the new language (for
    /// the next new game or load; a running game keeps its content).
    fn follow_language(&mut self) {
        let now = razdor::i18n::lang();
        if now != self.lang {
            self.lang = now;
            self.demo = Arc::new(Content::builtin());
        }
    }

    /// A frame of the editor; test play starts a game on the edited map.
    fn editor_frame(&mut self) {
        if hotkeys::allowed(hotkeys::Place::Editor, hotkeys::Global::Language, self.guard())
            && !widgets::popup_open()
            && is_key_pressed(language::KEY)
        {
            language::toggle();
        }
        let Some(ed) = self.editor.as_mut() else {
            self.open_editor();
            return;
        };
        match ed.frame(&self.assets) {
            editor::EditorAction::None => {}
            editor::EditorAction::Exit => {
                self.editor = None;
                self.screen = Screen::MainMenu;
            }
            editor::EditorAction::TestPlay { scenario, content, class } => {
                let mut game = Game::from_scenario(content, &scenario, class, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64));
                game.set_hero_name("");
                self.dialogs.clear();
                self.message = Some(tr("Test play: Esc > Main menu returns to the editor.").to_string());
                self.map_view.reset();
                self.last_gold = None;
                self.game = Some(game);
                self.test_play = true;
                self.screen = Screen::WorldMap;
            }
        }
    }

    /// Loads save file `path`: the demo from the built-in data, a map from the install (the
    /// same map file only). A pending battle starts again; a pending question is asked again.
    fn load(&mut self, path: &std::path::Path) {
        let install = self.assets.dt.as_ref().zip(self.dt_content.clone()).map(|(d, content)| Install { dir: &d.install.dir, content });
        match save::load(path, self.demo.clone(), install.as_ref()) {
            Ok(mut game) => {
                self.dialogs.clear();
                self.message = None;
                self.load_error = None;
                self.map_view.reset();
                self.last_gold = None;
                if let Some(q) = game.pending_question() {
                    story::show(&game, &EventOutcome::Question(q), &mut self.message, &mut self.dialogs);
                }
                self.screen = if game.foe.is_some() {
                    Screen::Battle(Box::new(BattleView::new(game.start_battle())))
                } else {
                    Screen::WorldMap
                };
                self.game = Some(game);
            }
            Err(e) => self.load_error = Some(razdor::trf!("Cannot load: {e}.", e)),
        }
    }

    /// "Рестарт": the scenario under way from its beginning, with the same hero class and
    /// name (the map is found again by its file name).
    fn restart(&mut self) {
        let Some(old) = self.game.as_ref() else { return };
        let hero = old.hero_class().unwrap_or(razdor::rules::content::HeroClass::Knight);
        let name = old.hero_name.clone().unwrap_or_default();
        let game = match &old.origin {
            Some(save::ScenarioRef::Map { file, .. }) => {
                let (Some(e), Some(c)) = (self.scenarios.iter().find(|e| &e.file == file), self.dt_content.clone()) else {
                    self.message = Some(tr("The map of this game is not in the install.").into());
                    self.screen = Screen::WorldMap;
                    return;
                };
                screens::start_game(&self.demo, Some((e, &c)), hero, &name)
            }
            _ => screens::start_game(&self.demo, None, hero, &name),
        };
        self.game = Some(game);
        self.dialogs.clear();
        self.message = None;
        self.map_view.reset();
        self.last_gold = None;
        self.screen = Screen::WorldMap;
    }

    /// Before the process ends: the music stops and the settings are written.
    pub fn shutdown(&mut self) {
        self.audio.shutdown();
    }

    /// The music the current screen wants.
    fn mood(&self) -> Mood {
        match &self.screen {
            Screen::MainMenu | Screen::Authors(_) | Screen::Options | Screen::ScenarioSelect | Screen::ClassSelect { .. } | Screen::Editor => Mood::Menu,
            Screen::Load(v) if v.back == saves::Back::Title || self.game.is_none() => Mood::Menu,
            Screen::Battle(_) => Mood::Battle,
            Screen::GameOver => Mood::Lost,
            Screen::Victory => Mood::Won,
            _ if self.game.is_some() => Mood::Map,
            _ => Mood::Menu,
        }
    }

    /// Sounds that follow from what changed this frame (a window opened, a battle began, gold
    /// came in, a dialog appeared), then the audio frame.
    fn sounds(&mut self) {
        let now = std::mem::discriminant(&self.screen);
        if self.last_screen != Some(now) {
            match self.screen {
                Screen::Battle(_) => audio::cue(Cue::BattleHorn),
                Screen::Building(_)
                | Screen::Squad { .. }
                | Screen::Journal(_)
                | Screen::Spellbook { .. }
                | Screen::Menu(_)
                | Screen::Settings
                | Screen::Save(_)
                | Screen::Load(_) => audio::cue(Cue::Panel),
                _ => {}
            }
        }
        self.last_screen = Some(now);
        let new_game = matches!(self.screen, Screen::ScenarioSelect | Screen::ClassSelect { .. });
        let gold = self.game.as_ref().filter(|_| !new_game).map(|g| g.gold);
        if let (Some(before), Some(after)) = (self.last_gold, gold) {
            if after > before {
                audio::cue(Cue::Gold);
            }
        }
        self.last_gold = gold;
        if let Some(d) = self.dialogs.front_mut().filter(|d| !d.cued) {
            d.cued = true;
            audio::cue(if d.event.is_some() { Cue::Event } else { Cue::Panel });
        }
        // N: music on/off (not while typing or answering a question: N is its "No").
        if !self.help && hotkeys::shortcuts_allowed(self.guard()) && is_key_pressed(KeyCode::N) {
            self.audio.settings.music_muted = !self.audio.settings.music_muted;
        }
        let mood = self.mood();
        self.audio.frame(mood);
    }

    /// The current screen, by what its keys do.
    fn place(&self) -> hotkeys::Place {
        use hotkeys::Place;
        match &self.screen {
            Screen::MainMenu | Screen::Authors(_) | Screen::Options | Screen::ScenarioSelect => Place::Title,
            Screen::ClassSelect { .. } => Place::ClassSelect,
            Screen::WorldMap => Place::WorldMap,
            Screen::Building(_) => Place::Building,
            Screen::Squad { .. } => Place::Army,
            Screen::Battle(v) => Place::Battle { deploying: v.deploying() },
            Screen::Journal(_) => Place::Journal,
            Screen::Spellbook { .. } => Place::Spellbook,
            Screen::Menu(_) | Screen::Settings => Place::Menu,
            Screen::Save(_) => Place::Save,
            Screen::Load(_) => Place::Load,
            Screen::GameOver | Screen::Victory => Place::End,
            Screen::Editor => Place::Editor,
        }
    }

    /// What stands in the way of shortcut keys this frame.
    fn guard(&self) -> hotkeys::Guard {
        hotkeys::Guard {
            typing: hotkeys::typing(self.place(), widgets::typing()),
            dialog: !self.dialogs.is_empty(),
            game: self.game.is_some(),
            foe: self.game.as_ref().is_some_and(|g| g.foe.is_some()),
        }
    }

    /// F5: writes the quick save (a manual save named "Quick save", replacing the last).
    fn quick_save(&mut self) {
        let Some(game) = &self.game else { return };
        self.message = Some(match save::default_dir() {
            None => tr("No data folder for saves: set RAZDOR_SAVE_DIR.").to_string(),
            Some(dir) => match save::quick_save(&dir, game) {
                Ok(_) => tr("Quick save written (F9 loads it).").to_string(),
                Err(e) => razdor::trf!("Not saved: {e}.", e),
            },
        });
    }

    /// F9: loads the quick save, if there is one.
    fn quick_load(&mut self) {
        match save::default_dir().and_then(|d| save::quick_save_path(&d)) {
            Some(path) => {
                self.load(&path);
                self.message = Some(self.load_error.take().unwrap_or_else(|| tr("Quick save loaded.").to_string()));
            }
            None => self.message = Some(tr("No quick save yet: F5 writes one.").to_string()),
        }
    }

    /// The current screen's name, for the frame timer (`RAZDOR_PROFILE`).
    pub fn screen_name(&self) -> &'static str {
        match self.screen {
            Screen::MainMenu => "main menu",
            Screen::Authors(_) => "authors",
            Screen::Options => "options",
            Screen::ScenarioSelect => "scenario select",
            Screen::ClassSelect { .. } => "class select",
            Screen::WorldMap => "world map",
            Screen::Building(_) => "building",
            Screen::Squad { .. } => "army",
            Screen::Battle(_) => "battle",
            Screen::Journal(_) => "journal",
            Screen::Spellbook { .. } => "spell book",
            Screen::Menu(_) => "menu",
            Screen::Settings => "settings",
            Screen::Save(_) => "save",
            Screen::Load(_) => "load",
            Screen::GameOver => "game over",
            Screen::Victory => "victory",
            Screen::Editor => "editor",
        }
    }

    pub fn frame(&mut self) {
        chrome::begin_frame();
        self.follow_language();
        self.sounds();
        if matches!(self.screen, Screen::Editor) {
            self.editor_frame();
            return;
        }
        // A dialog or the key list on top: the screen below is drawn but takes no input.
        let place = self.place();
        let guard = self.guard();
        widgets::set_input_blocked(!self.dialogs.is_empty() || self.help);
        let mut restart = false;
        let mut next = match (&mut self.screen, &mut self.game) {
            (Screen::MainMenu, _) => match main_menu::frame() {
                Some(main_menu::Pick::NewGame) => Some(Screen::ScenarioSelect),
                Some(main_menu::Pick::Load) => Some(Screen::Load(saves::LoadView::new(saves::Back::Title))),
                Some(main_menu::Pick::Editor) => Some(Screen::Editor),
                Some(main_menu::Pick::Exit) => {
                    self.quit = true;
                    None
                }
                Some(main_menu::Pick::Options) => Some(Screen::Options),
                Some(main_menu::Pick::Authors) => Some(Screen::Authors(macroquad::prelude::get_time())),
                None => None,
            },
            (Screen::Authors(started), _) => main_menu::authors(*started).then_some(Screen::MainMenu),
            (Screen::Options, _) => main_menu::options(&mut self.audio.settings).then_some(Screen::MainMenu),
            (Screen::ScenarioSelect, _) => new_game::scenario_select(&self.scenarios, self.dt_content.is_some()),
            (Screen::ClassSelect { scenario }, game) => {
                let pick = scenario.and_then(|i| Some((self.scenarios.get(i)?, self.dt_content.clone()?)));
                new_game::class_select(game, &self.demo, pick, &self.assets)
            }
            (Screen::WorldMap, Some(game)) => {
                world_view::frame(game, &self.assets, &mut self.map_view, &mut self.message, &mut self.dialogs)
            }
            (Screen::Building(view), Some(game)) => {
                building_view::frame(game, &self.assets, view, &mut self.message, &mut self.dialogs)
            }
            (Screen::Squad { selected, scroll, back }, Some(game)) => {
                items_view::squad(game, &self.assets, selected, scroll, back, &mut self.message)
            }
            (Screen::Battle(view), Some(game)) => view.frame(game, &self.assets, &mut self.message, &mut self.dialogs),
            (Screen::Journal(view), Some(game)) => story::journal(game, &self.assets, view),
            (Screen::Spellbook { selected }, Some(game)) => {
                spellbook::frame(game, &self.assets, selected, &mut self.message, &mut self.dialogs)
            }
            (Screen::Menu(asking), Some(game)) => match saves::exit_window(game, &self.assets, asking) {
                (Some(saves::ExitChoice::Quit), _) => {
                    self.quit = true;
                    None
                }
                (Some(saves::ExitChoice::MainMenu), _) => Some(Screen::MainMenu),
                (Some(saves::ExitChoice::Restart), _) => {
                    restart = true;
                    None
                }
                (None, next) => next,
            },
            (Screen::Settings, Some(game)) => {
                world_view::backdrop_lit(game, &self.assets, Some(game_bar::BarButton::Settings));
                main_menu::options_window(&mut self.audio.settings).then_some(Screen::WorldMap)
            }
            (Screen::Save(view), Some(game)) => saves::save_screen(game, &self.assets, view, &mut self.message),
            (Screen::Load(view), game) => saves::load_screen(game.as_ref(), &self.assets, view, &mut self.pending_load, &self.load_error),
            (Screen::GameOver, game) => screens::game_over(game),
            (Screen::Victory, game) => screens::victory(game, &self.scenarios, self.dt_content.clone()),
            (Screen::Editor, _) => None,
            (_, None) => Some(Screen::MainMenu),
        };
        widgets::set_input_blocked(false);
        // "Варианты выхода из битвы" chose.
        if let Screen::Battle(v) = &mut self.screen {
            match v.exit.take() {
                Some(saves::ExitChoice::Quit) => self.quit = true,
                Some(saves::ExitChoice::MainMenu) => next = Some(Screen::MainMenu),
                Some(saves::ExitChoice::Restart) => restart = true,
                None => {}
            }
        }
        if restart {
            self.restart();
            return;
        }
        // F1: the key list; F5 / F9: quick save and load (when the screen did not move on).
        let pressed = |k: hotkeys::Global| next.is_none() && hotkeys::allowed(place, k, guard) && is_key_pressed(k.key());
        if self.help {
            if hotkeys::help_overlay(place) {
                self.help = false;
            }
        } else if pressed(hotkeys::Global::Help) {
            self.help = true;
        } else if pressed(hotkeys::Global::Language) {
            language::toggle();
        } else if pressed(hotkeys::Global::QuickSave) {
            self.quick_save();
        } else if pressed(hotkeys::Global::QuickLoad) {
            self.quick_load();
            return;
        }
        if let Some(d) = self.dialogs.front() {
            if let Some(close) = dialog::draw(d, &self.assets) {
                let asked = self.dialogs.pop_front().is_some_and(|d| d.question);
                // A scenario question: the answer goes to the event engine.
                if let (true, Some(game)) = (asked, self.game.as_mut()) {
                    let events = game.answer_question(close == Close::Yes);
                    let after = world_view::handle_events(game, events, &mut self.message, &mut self.dialogs);
                    next = next.or(after);
                }
            }
        }
        // A victory or defeat event ends the game once its window is read.
        if next.is_none() && self.dialogs.is_empty() {
            let end = self.game.as_ref().and_then(Game::script_end);
            match end {
                Some(ScriptEnd::Victory(_)) if !matches!(self.screen, Screen::Victory) => next = Some(Screen::Victory),
                Some(ScriptEnd::Defeat(_)) if !matches!(self.screen, Screen::GameOver) => next = Some(Screen::GameOver),
                _ => {}
            }
        }
        // The noon report asks for an autosave, named by the date.
        if let Some(g) = self.game.as_mut() {
            if let Some(name) = g.autosave_due.take() {
                saves::autosave(g, &name);
            }
        }
        if let Some(path) = self.pending_load.take() {
            self.load(&path);
            return;
        }
        // Leaving a test play (main menu, or "new game" on an end screen) returns to the editor.
        if self.test_play && matches!(next, Some(Screen::MainMenu | Screen::ScenarioSelect)) {
            self.test_play = false;
            self.game = None;
            self.dialogs.clear();
            self.message = None;
            self.screen = Screen::Editor;
            return;
        }
        if let Some(next) = next {
            if matches!(next, Screen::Load(_)) {
                self.load_error = None;
            }
            if matches!(next, Screen::MainMenu | Screen::ScenarioSelect) {
                self.dialogs.clear();
            }
            if matches!(next, Screen::WorldMap) && !matches!(self.screen, Screen::WorldMap) {
                self.map_view.reset();
            }
            if matches!(self.screen, Screen::ClassSelect { .. }) {
                self.message = None;
            }
            self.screen = next;
        }
        // A new game may run on other content: draw its pictures.
        if let Some(g) = &self.game {
            if !Arc::ptr_eq(&g.content, self.assets.content()) {
                self.assets.set_content(g.content.clone());
            }
        }
    }
}
