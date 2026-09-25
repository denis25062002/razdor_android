//! macroquad presentation layer. Reads rules state and calls rules methods.
pub mod assets;
pub mod battle_view;
pub mod building_view;
pub mod dialog;
pub mod dt_art;
pub mod items_view;
pub mod minimap;
pub mod saves;
pub mod screens;
pub mod spellbook;
pub mod story;
pub mod widgets;
pub mod world_view;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use razdor::dt::dtm::Scenario;
use razdor::rules::content::Content;
use razdor::rules::events::EventOutcome;
use razdor::rules::game::Game;
use razdor::rules::save::{self, Install};
use razdor::rules::script::ScriptEnd;

use assets::Assets;
use battle_view::BattleView;
use building_view::BuildingView;
use dialog::{Close, Dialog};
use world_view::MapView;

pub enum Screen {
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
    /// The quest journal, with the selected line.
    Journal { selected: usize },
    /// The spell book, with the selected cell.
    Spellbook { selected: usize },
    /// The Esc menu, and the save and load windows.
    Menu,
    Save(saves::SaveView),
    Load(saves::LoadView),
    GameOver,
    Victory,
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
        App {
            assets,
            demo,
            dt_content,
            scenarios,
            game: None,
            screen: Screen::ScenarioSelect,
            message: None,
            dialogs: VecDeque::new(),
            map_view: MapView::default(),
            pending_load: None,
            load_error: None,
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
            Err(e) => self.load_error = Some(format!("Cannot load: {e}.")),
        }
    }

    pub fn frame(&mut self) {
        // A dialog on top: the screen below is drawn but takes no input.
        widgets::set_input_blocked(!self.dialogs.is_empty());
        let mut next = match (&mut self.screen, &mut self.game) {
            (Screen::ScenarioSelect, _) => screens::scenario_select(&self.scenarios, self.dt_content.is_some()),
            (Screen::ClassSelect { scenario }, game) => {
                let pick = scenario.and_then(|i| Some((self.scenarios.get(i)?, self.dt_content.clone()?)));
                screens::class_select(game, &self.demo, pick, &self.assets)
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
            (Screen::Journal { selected }, Some(game)) => story::journal(game, &self.assets, selected),
            (Screen::Spellbook { selected }, Some(game)) => {
                spellbook::frame(game, &self.assets, selected, &mut self.message, &mut self.dialogs)
            }
            (Screen::Menu, Some(game)) => saves::menu(game, &self.assets),
            (Screen::Save(view), Some(game)) => saves::save_screen(game, &self.assets, view, &mut self.message),
            (Screen::Load(view), game) => saves::load_screen(game.as_ref(), &self.assets, view, &mut self.pending_load, &self.load_error),
            (Screen::GameOver, game) => screens::game_over(game),
            (Screen::Victory, game) => screens::victory(game),
            (_, None) => Some(Screen::ScenarioSelect),
        };
        widgets::set_input_blocked(false);
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
        if let Some(next) = next {
            if matches!(next, Screen::Load(_)) {
                self.load_error = None;
            }
            if matches!(next, Screen::ScenarioSelect) {
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
