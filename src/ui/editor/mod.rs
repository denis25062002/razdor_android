//! The map editor window: toolbar, map canvas, tool palette, minimap, property panels and
//! dialogs. All editing goes through the pure model in `razdor::editor`; this module turns
//! mouse and keys into its calls and draws.
//!
//! Layout: toolbar on top, the tool palette with the minimap on the right, the selected
//! record's panel over the left of the map, a status line at the bottom.

mod canvas;
mod form;
mod palette_panel;
mod props;
mod settings;

use std::path::PathBuf;
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::dt::dtm::Scenario;
use razdor::dt::install::{self, MapEntry};
use razdor::editor::defaults::MAP_SIZES;
use razdor::editor::files::{self, Consent, Destination, SaveBlock};
use razdor::editor::palette::{object_class_label, SURFACE_LABELS};
use razdor::editor::validate::has_errors;
use razdor::editor::{Command, EditorDoc, Issue, Names, NewMap, Origin, Palette, Place, SaveError, Severity, Target, Tool, ToolState};
use razdor::rules::content::{Content, HeroClass};

use crate::ui::assets::Assets;
use crate::ui::widgets::*;

use canvas::{Cam, Overlays, Overview};
use palette_panel::{PaletteState, TOOL_KEYS};
use props::{Ctx, PanelState};
use settings::{SettingsAction, SettingsState};

const TOP: f32 = 44.0;
const RIGHT_W: f32 = 300.0;
const STATUS_H: f32 = 26.0;
const PANEL_W: f32 = 390.0;
const MINIMAP_H: f32 = 170.0;

/// What the editor asks the app to do.
pub enum EditorAction {
    None,
    /// Back to the title screen.
    Exit,
    /// Play the edited scenario with this content and hero class; come back afterwards.
    TestPlay { scenario: Box<Scenario>, content: Arc<Content>, class: HeroClass },
}

/// What a confirmation leads to.
#[derive(Clone)]
enum Then {
    New(NewMap),
    Open(PathBuf),
    Exit,
    Save { name: String, dest: Destination, consent: Consent },
}

enum Modal {
    NewMap { size: usize, w: u32, h: u32, fill: u8 },
    Open { path: String, scroll: usize },
    SaveAs { name: String },
    Confirm { message: String, then: Then },
    Issues { scroll: usize },
    Settings,
    TestPlay,
}

pub struct EditorScreen {
    doc: EditorDoc,
    tools: ToolState,
    pal: PaletteState,
    cam: Option<Cam>,
    overview: Overview,
    overlays: Overlays,
    panel: PanelState,
    settings: SettingsState,
    modal: Option<Modal>,
    status: Option<String>,
    issues: Vec<Issue>,
    palette: Palette,
    /// Names from the install (checks and pickers); `None` without one.
    install_names: Option<Names>,
    /// Names of the content test play uses (the install's, else the demo's).
    play_names: Names,
    play_content: Arc<Content>,
    user_dir: Option<PathBuf>,
    game_dir: Option<PathBuf>,
    /// The left button went down on the map (a press, drags and a release follow).
    pressing: bool,
    last_cell: Option<(i32, i32)>,
    panning: Option<Vec2>,
}

fn ctrl() -> bool {
    is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
}

fn shift() -> bool {
    is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift)
}

impl EditorScreen {
    /// A new 50×50 map. `dt_content` is the install's content (names and test play), `demo`
    /// the built-in content test play falls back to.
    pub fn new(assets: &Assets, dt_content: Option<Arc<Content>>, demo: Arc<Content>) -> EditorScreen {
        let art = assets.dt.as_ref();
        let palette = art.map(|a| Palette::from_sprites(a.objects())).filter(|p| !p.buildings.is_empty()).unwrap_or_else(Palette::fallback);
        let game_dir = art.and_then(|a| install::find_path(&a.install.dir, install::MAPS_DIR).ok());
        let play_content = dt_content.clone().unwrap_or(demo);
        let mut pal = PaletteState::default();
        pal.fit(&palette);
        EditorScreen {
            doc: EditorDoc::new_map(NewMap::default()),
            tools: ToolState::default(),
            pal,
            cam: None,
            overview: Overview::default(),
            overlays: Overlays { grid: false, cover: false, patrols: false },
            panel: PanelState::default(),
            settings: SettingsState::default(),
            modal: None,
            status: Some("New 50 x 50 map. Maps are saved to your own folder; see Save as.".into()),
            issues: Vec::new(),
            palette,
            install_names: dt_content.as_deref().map(Names::from_content),
            play_names: Names::from_content(&play_content),
            play_content,
            user_dir: files::user_maps_dir(),
            game_dir,
            pressing: false,
            last_cell: None,
            panning: None,
        }
    }

    fn names(&self) -> &Names {
        self.install_names.as_ref().unwrap_or(&self.play_names)
    }

    fn install_maps(&self, assets: &Assets) -> Vec<MapEntry> {
        assets.dt.as_ref().map(|a| a.install.maps.clone()).unwrap_or_default()
    }

    fn view_rect() -> Rect {
        Rect::new(0.0, TOP, screen_width() - RIGHT_W, screen_height() - TOP - STATUS_H)
    }

    fn set_doc(&mut self, doc: EditorDoc) {
        self.doc = doc;
        let tool = self.tools.tool;
        self.tools = ToolState::default();
        self.tools.set_tool(tool);
        self.cam = None;
        self.issues.clear();
    }

    fn open_file(&mut self, path: PathBuf) {
        match EditorDoc::open(&path, self.game_dir.as_deref()) {
            Ok(d) => {
                let note = if matches!(d.origin, Origin::Game(_)) { " (a game map: saving goes to your own folder)" } else { "" };
                self.status = Some(format!("Opened {}{note}.", path.display()));
                self.set_doc(d);
            }
            Err(e) => self.status = Some(format!("Cannot open {}: {e}", path.display())),
        }
    }

    /// Runs a confirmed (or unneeded) follow-up.
    fn run(&mut self, then: Then) -> EditorAction {
        match then {
            Then::New(o) => {
                self.set_doc(EditorDoc::new_map(o));
                self.status = Some(format!("New {} x {} map.", o.width, o.height));
            }
            Then::Open(p) => self.open_file(p),
            Then::Exit => return EditorAction::Exit,
            Then::Save { name, dest, consent } => self.save(&name, dest, consent),
        }
        EditorAction::None
    }

    /// `then`, after asking about unsaved changes if there are any.
    fn guarded(&mut self, then: Then) -> EditorAction {
        if self.doc.dirty() {
            self.modal = Some(Modal::Confirm { message: "The map has unsaved changes. Discard them?".into(), then });
            EditorAction::None
        } else {
            self.run(then)
        }
    }

    /// Saves under `name` to `dest`, asking for every confirmation the rules need first.
    fn save(&mut self, name: &str, dest: Destination, consent: Consent) {
        let current = self.doc.saved_path.clone();
        match files::plan_save(name, dest, self.user_dir.as_deref(), self.game_dir.as_deref(), current.as_deref(), consent) {
            Ok(path) => match self.doc.save_to(&path, self.install_names.as_ref(), Some(&self.palette)) {
                Ok(()) => self.status = Some(format!("Saved {}.", path.display())),
                Err(SaveError::Invalid(issues)) => {
                    self.issues = issues;
                    self.modal = Some(Modal::Issues { scroll: 0 });
                    self.status = Some("Not saved: the map has errors.".into());
                }
                Err(e) => self.status = Some(format!("Not saved: {e}")),
            },
            Err(block) => {
                let mut next = consent;
                match &block {
                    SaveBlock::ConfirmGameFolder(_) => next.game_folder = true,
                    SaveBlock::ConfirmReplaceGameMap(_) => next.replace_game_map = true,
                    SaveBlock::ConfirmReplaceOwnMap(_) => next.replace_own_map = true,
                    SaveBlock::BadName(_) | SaveBlock::NoFolder => {
                        self.status = Some(format!("Not saved: {block}."));
                        return;
                    }
                }
                let message = block.to_string();
                self.modal = Some(Modal::Confirm { message, then: Then::Save { name: name.to_string(), dest, consent: next } });
            }
        }
    }

    /// Ctrl+S: the file saved to before (the game folder asks again), else "save as".
    fn quick_save(&mut self) {
        match self.doc.saved_path.clone() {
            Some(p) => {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let in_game = self.game_dir.as_deref().is_some_and(|g| files::is_inside(&p, g));
                if in_game {
                    self.save(&name, Destination::GameFolder, Consent::default());
                } else if self.user_dir.as_deref().is_some_and(|u| files::is_inside(&p, u)) {
                    self.save(&name, Destination::UserFolder, Consent::default());
                } else {
                    // A file opened from elsewhere: saved where it is.
                    match self.doc.save_to(&p, self.install_names.as_ref(), Some(&self.palette)) {
                        Ok(()) => self.status = Some(format!("Saved {}.", p.display())),
                        Err(SaveError::Invalid(issues)) => {
                            self.issues = issues;
                            self.modal = Some(Modal::Issues { scroll: 0 });
                        }
                        Err(e) => self.status = Some(format!("Not saved: {e}")),
                    }
                }
            }
            None => self.modal = Some(Modal::SaveAs { name: self.doc.suggested_name() }),
        }
    }

    fn apply(&mut self, cmd: Command, key: &str) {
        let key = (!key.is_empty()).then_some(key);
        if let Err(e) = self.doc.apply_merging(cmd, key) {
            self.status = Some(format!("Refused: {e}."));
        }
        self.tools.check_selection(&self.doc);
    }

    fn check(&mut self) {
        self.issues = self.doc.issues(self.install_names.as_ref(), Some(&self.palette));
        self.modal = Some(Modal::Issues { scroll: 0 });
    }

    /// Centres the view on what an issue is about and selects it.
    fn go_to(&mut self, place: Place) {
        let s = &self.doc.scenario;
        let (target, cell) = match place {
            Place::Building(id) => (Some(Target::Building(id)), s.building(id).map(|b| (b.x, b.y))),
            Place::Army(id) => (Some(Target::Army(id)), s.army(id).map(|a| (a.x, a.y))),
            Place::Point(id) => (Some(Target::Point(id)), s.points.get(id as usize - 1).map(|p| (p.x, p.y))),
            Place::Object(i) => (None, s.objects.get(i).map(|o| (o.x, o.y))),
            Place::Hero(k) => (None, s.header.heroes.get(k).map(|h| (h.x, h.y))),
            Place::Settings | Place::Event(_) => {
                self.modal = Some(Modal::Settings);
                return;
            }
            Place::Map => (None, None),
        };
        if target.is_some() {
            self.tools.selected = target;
        }
        if let (Some((x, y)), Some(cam)) = (cell, self.cam.as_mut()) {
            cam.centre = vec2(x as f32 + 0.5, y as f32 + 0.5);
        }
        self.modal = None;
    }

    fn start_test_play(&mut self, class: HeroClass) -> EditorAction {
        let issues = self.doc.issues(Some(&self.play_names), None);
        if has_errors(&issues) {
            self.issues = issues;
            self.modal = Some(Modal::Issues { scroll: 0 });
            self.status = Some("Fix the errors before test play.".into());
            return EditorAction::None;
        }
        self.modal = None;
        EditorAction::TestPlay { scenario: Box::new(self.doc.scenario.clone()), content: self.play_content.clone(), class }
    }

    /// One frame of the editor.
    pub fn frame(&mut self, assets: &Assets) -> EditorAction {
        clear_background(Color::from_rgba(12, 12, 11, 255));
        fields_begin_frame();
        let art = assets.dt.as_ref();
        let view = Self::view_rect();
        let s = &self.doc.scenario;
        let cam = self.cam.get_or_insert_with(|| {
            let mut c = Cam::new(view, s.width(), s.height());
            c.zoom = 0.75;
            c
        });
        cam.view = view;
        let modal_open = self.modal.is_some();
        // A dialog or an open list takes all input.
        set_input_blocked(modal_open || popup_open());

        let mut action = EditorAction::None;
        if !modal_open && !typing() && !popup_open() {
            action = self.shortcuts();
        }
        let overview = self.overview.texture(&self.doc);
        let panel_rect = self.tools.selected.map(|_| Rect::new(8.0, TOP + 8.0, PANEL_W, view.h - 16.0));
        self.canvas_input(view, panel_rect);
        let cam = self.cam.expect("set above");
        canvas::draw_map(&self.doc, art, &cam, overview.as_ref());
        let hover = (!modal_open && view.contains(Vec2::from(mouse_position()))).then(|| cam.cell_at(Vec2::from(mouse_position())));
        canvas::draw_overlays(&self.doc, &self.tools, &self.palette, art, &cam, hover, &self.overlays);

        // Right column: minimap and tools.
        let rx = screen_width() - RIGHT_W;
        let mini = Rect::new(rx + 6.0, TOP + 6.0, RIGHT_W - 12.0, MINIMAP_H);
        if let Some(at) = canvas::minimap(&self.doc, overview.as_ref(), &cam, mini) {
            if let Some(c) = self.cam.as_mut() {
                c.centre = at;
            }
        }
        let tools_rect = Rect::new(rx, mini.bottom() + 6.0, RIGHT_W, screen_height() - mini.bottom() - 6.0 - STATUS_H);
        palette_panel::tool_panel(&mut self.pal, &mut self.tools, &self.palette, art, tools_rect);

        // The selected record's panel.
        if let (Some(t), Some(pr)) = (self.tools.selected, panel_rect) {
            let ctx = Ctx { names: self.install_names.as_ref().unwrap_or(&self.play_names), palette: &self.palette, content: Some(&*self.play_content) };
            let s = &self.doc.scenario;
            let edit = match t {
                Target::Building(id) => props::building_panel(&mut self.panel, s, id, &ctx, pr),
                Target::Army(id) => props::army_panel(&mut self.panel, s, id, &ctx, pr),
                Target::Point(id) => props::point_panel(&mut self.panel, s, id, pr),
            };
            if small_button(pr.right() - 34.0, pr.y + 6.0, 26.0, 24.0, "x", true) {
                self.tools.selected = None;
            } else if let Some((cmd, key)) = edit {
                self.apply(cmd, &key);
            }
        }

        let bar = self.toolbar(assets);
        if !matches!(bar, EditorAction::None) {
            action = bar;
        }
        self.status_line(hover);

        // Dialogs on top.
        set_input_blocked(popup_open());
        if self.modal.is_some() {
            let m = self.modal_frame(assets);
            if !matches!(m, EditorAction::None) {
                action = m;
            }
        }
        set_input_blocked(false);
        draw_popup();
        fields_end_frame();
        action
    }

    fn shortcuts(&mut self) -> EditorAction {
        let c = ctrl();
        if c && is_key_pressed(KeyCode::Z) {
            if shift() {
                self.doc.redo();
            } else {
                self.doc.undo();
            }
            self.tools.check_selection(&self.doc);
        } else if c && is_key_pressed(KeyCode::Y) {
            self.doc.redo();
            self.tools.check_selection(&self.doc);
        } else if c && is_key_pressed(KeyCode::S) {
            if shift() {
                self.modal = Some(Modal::SaveAs { name: self.doc.suggested_name() });
            } else {
                self.quick_save();
            }
        } else if c && is_key_pressed(KeyCode::O) {
            self.modal = Some(Modal::Open { path: String::new(), scroll: 0 });
        } else if c && is_key_pressed(KeyCode::N) {
            self.modal = Some(Modal::NewMap { size: 0, w: 50, h: 50, fill: 6 });
        } else if !c {
            if is_key_pressed(KeyCode::Delete) {
                self.tools.delete_selected(&mut self.doc);
            }
            if is_key_pressed(KeyCode::Escape) {
                if self.tools.selected.is_some() {
                    self.tools.selected = None;
                } else {
                    self.tools.set_tool(Tool::Select);
                }
            }
            for (k, (_, key)) in TOOL_KEYS.iter().enumerate() {
                if is_key_pressed(*key) {
                    self.tools.set_tool(self.pal.tool(k));
                }
            }
            if is_key_pressed(KeyCode::G) {
                self.overlays.grid = !self.overlays.grid;
            }
            if is_key_pressed(KeyCode::H) {
                self.overlays.cover = !self.overlays.cover;
            }
            if is_key_pressed(KeyCode::R) {
                self.overlays.patrols = !self.overlays.patrols;
            }
            let s = &self.doc.scenario;
            let (w, h) = (s.width(), s.height());
            if let Some(cam) = self.cam.as_mut() {
                let step = 600.0 * get_frame_time();
                let mut d = Vec2::ZERO;
                if is_key_down(KeyCode::Left) {
                    d.x += step;
                }
                if is_key_down(KeyCode::Right) {
                    d.x -= step;
                }
                if is_key_down(KeyCode::Up) {
                    d.y += step;
                }
                if is_key_down(KeyCode::Down) {
                    d.y -= step;
                }
                if d != Vec2::ZERO {
                    cam.pan(d);
                    cam.clamp(w, h);
                }
                if is_key_pressed(KeyCode::Equal) || is_key_pressed(KeyCode::KpAdd) {
                    cam.zoom_at(1.25, cam.view.center());
                }
                if is_key_pressed(KeyCode::Minus) || is_key_pressed(KeyCode::KpSubtract) {
                    cam.zoom_at(0.8, cam.view.center());
                }
                if is_key_pressed(KeyCode::Home) {
                    cam.fit(w, h);
                }
            }
        }
        EditorAction::None
    }

    /// Mouse on the map: tools with the left button, panning with the right or middle one,
    /// zoom with the wheel.
    fn canvas_input(&mut self, view: Rect, panel: Option<Rect>) {
        let m = Vec2::from(mouse_position());
        let over = view.contains(m) && !panel.is_some_and(|p| p.contains(m)) && !input_blocked();
        let (w, h) = (self.doc.scenario.width(), self.doc.scenario.height());
        let Some(cam) = self.cam.as_mut() else { return };
        if over {
            let wh = mouse_wheel().1;
            if wh != 0.0 {
                cam.zoom_at(if wh > 0.0 { 1.12 } else { 1.0 / 1.12 }, m);
            }
        }
        // Panning.
        let pan_button = is_mouse_button_down(MouseButton::Right) || is_mouse_button_down(MouseButton::Middle);
        if pan_button {
            if let Some(last) = self.panning {
                cam.pan(m - last);
                cam.clamp(w, h);
                self.panning = Some(m);
            } else if over && (is_mouse_button_pressed(MouseButton::Right) || is_mouse_button_pressed(MouseButton::Middle)) {
                self.panning = Some(m);
            }
        } else {
            self.panning = None;
        }
        let cell = cam.cell_at(m);
        if over && is_mouse_button_pressed(MouseButton::Left) {
            clear_focus();
            self.pressing = true;
            self.last_cell = Some(cell);
            self.tools.press(&mut self.doc, &self.palette, cell);
            self.after_tool();
        } else if self.pressing {
            if is_mouse_button_down(MouseButton::Left) {
                if self.last_cell != Some(cell) {
                    self.last_cell = Some(cell);
                    self.tools.drag_to(&mut self.doc, cell);
                }
            } else {
                self.pressing = false;
                self.tools.release(&mut self.doc, cell);
                self.after_tool();
            }
        }
    }

    fn after_tool(&mut self) {
        if let Some(m) = self.tools.message.take() {
            self.status = Some(format!("Refused: {m}."));
        }
        if self.tools.selected.is_some() && !matches!(self.tools.tool, Tool::Select) {
            // A record just placed: its panel opens at the first tab.
            self.panel.tab = 0;
        }
    }

    fn toolbar(&mut self, _assets: &Assets) -> EditorAction {
        let w = screen_width();
        draw_rectangle(0.0, 0.0, w, TOP, Color::new(0.14, 0.12, 0.1, 1.0));
        draw_line(0.0, TOP, w, TOP, 1.0, DIM);
        let mut x = 6.0;
        let mut b = |label: &str, bw: f32, enabled: bool| {
            let hit = small_button(x, 8.0, bw, 28.0, label, enabled);
            x += bw + 4.0;
            hit
        };
        let mut action = EditorAction::None;
        if b("New", 50.0, true) {
            self.modal = Some(Modal::NewMap { size: 0, w: 50, h: 50, fill: 6 });
        }
        if b("Open", 56.0, true) {
            self.modal = Some(Modal::Open { path: String::new(), scroll: 0 });
        }
        if b("Save", 52.0, true) {
            self.quick_save();
        }
        if b("Save as", 72.0, true) {
            self.modal = Some(Modal::SaveAs { name: self.doc.suggested_name() });
        }
        if b("Save to game folder", 160.0, self.game_dir.is_some()) {
            let name = self.doc.suggested_name();
            self.save(&name, Destination::GameFolder, Consent::default());
        }
        let (cu, cr) = (self.doc.can_undo(), self.doc.can_redo());
        if b("Undo", 54.0, cu) {
            self.doc.undo();
            self.tools.check_selection(&self.doc);
        }
        if b("Redo", 54.0, cr) {
            self.doc.redo();
            self.tools.check_selection(&self.doc);
        }
        if b("Settings", 80.0, true) {
            self.modal = Some(Modal::Settings);
        }
        if b("Check", 60.0, true) {
            self.check();
        }
        if b("Test play", 84.0, true) {
            self.modal = Some(Modal::TestPlay);
        }
        if b("Exit", 50.0, true) {
            action = self.guarded(Then::Exit);
        }
        let s = &self.doc.scenario;
        let title = format!("{}{}  ({} x {})", if self.doc.dirty() { "* " } else { "" }, self.doc.suggested_name(), s.width(), s.height());
        let tw = measure(&title, 18.0).width;
        if x + tw + 12.0 < w {
            text(&title, w - tw - 10.0, 28.0, 18.0, INK);
        }
        action
    }

    fn status_line(&self, hover: Option<(i32, i32)>) {
        let (w, h) = (screen_width(), screen_height());
        let y = h - STATUS_H;
        draw_rectangle(0.0, y, w, STATUS_H, Color::new(0.08, 0.08, 0.075, 1.0));
        let s = &self.doc.scenario;
        let mut parts = Vec::new();
        if let Some((x, cy)) = hover.filter(|(x, y)| *x >= 0 && *y >= 0 && (*x as u32) < s.width() && (*y as u32) < s.height()) {
            let code = s.terrain_at(x as u32, cy as u32).unwrap_or(0);
            parts.push(format!("({x}, {cy}) {}", SURFACE_LABELS[code as usize & 15]));
            let objs: Vec<String> = self.doc.objects_at(x as u16, cy as u16).map(|o| format!("{} {}", object_class_label(o.class), o.sprite)).collect();
            if !objs.is_empty() {
                parts.push(objs.join(", "));
            }
        }
        parts.push(format!("{} buildings, {} armies, {} points, {} events", s.buildings.len(), s.armies.len(), s.points.len(), s.events.len()));
        text(&parts.join("   |   "), 8.0, y + 18.0, 16.0, DIM);
        if let Some(m) = &self.status {
            let tw = measure(m, 16.0).width;
            text(m, (w - RIGHT_W - tw - 10.0).max(w * 0.45), y + 18.0, 16.0, ACCENT);
        }
    }

    fn modal_frame(&mut self, assets: &Assets) -> EditorAction {
        let (sw, sh) = (screen_width(), screen_height());
        let mut action = EditorAction::None;
        let Some(modal) = self.modal.take() else { return action };
        if matches!(modal, Modal::Settings) {
            let names = self.names().clone();
            match settings::window(&mut self.settings, &self.doc.scenario, &names) {
                SettingsAction::Apply(cmd, key) => {
                    self.apply(cmd, &key);
                    self.modal = Some(Modal::Settings);
                }
                SettingsAction::PickStart(k) => {
                    self.tools.set_tool(Tool::HeroStart(k));
                    self.status = Some("Click the hero's start cell on the map.".into());
                }
                SettingsAction::Close => {}
                SettingsAction::None => self.modal = Some(Modal::Settings),
            }
            return action;
        }
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.55));
        let (w, h) = match &modal {
            Modal::Confirm { .. } => (560.0, 200.0),
            Modal::TestPlay => (560.0, 230.0),
            Modal::SaveAs { .. } => (620.0, 260.0),
            Modal::NewMap { .. } => (560.0, 330.0),
            _ => (720.0f32.min(sw - 40.0), (sh - 100.0).max(300.0)),
        };
        let r = Rect::new((sw - w) / 2.0, ((sh - h) / 2.0).max(10.0), w, h);
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.095, 0.09, 0.98));
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, ACCENT);
        let esc = !typing() && is_key_pressed(KeyCode::Escape);
        let (x, mut y) = (r.x + 20.0, r.y + 32.0);
        let cancel = |r: &Rect| button(r.right() - 140.0, r.bottom() - 54.0, 120.0, 40.0, "Cancel", true);
        let mut keep = true;
        let mut next = modal;
        match &mut next {
            Modal::Confirm { message, then } => {
                text("Please confirm", x, y, 22.0, ACCENT);
                for (i, line) in wrap(message, r.w - 40.0, 18.0).iter().take(4).enumerate() {
                    text(line, x, y + 32.0 + i as f32 * 22.0, 18.0, INK);
                }
                if button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, "Yes", true) {
                    let then = then.clone();
                    self.modal = None;
                    return self.run(then);
                }
                if cancel(&r) || esc {
                    keep = false;
                    self.status = Some("Cancelled.".into());
                }
            }
            Modal::TestPlay => {
                text("Test play: choose the hero", x, y, 22.0, ACCENT);
                let note = if self.install_names.is_some() { "With your install's units and rules." } else { "Without an install: the built-in demo's units." };
                text(note, x, y + 28.0, 16.0, DIM);
                text("Esc > Main menu in the game comes back here.", x, y + 48.0, 16.0, DIM);
                y += 70.0;
                for (k, class) in HeroClass::ALL.into_iter().enumerate() {
                    if button(x + k as f32 * 170.0, y, 160.0, 44.0, razdor::editor::palette::HERO_CLASSES[k], true) {
                        self.modal = None;
                        return self.start_test_play(class);
                    }
                }
                if cancel(&r) || esc {
                    keep = false;
                }
            }
            Modal::SaveAs { name } => {
                text("Save as", x, y, 22.0, ACCENT);
                let folder = self.user_dir.as_ref().map_or("(no data folder)".to_string(), |d| d.display().to_string());
                text(&format!("Into your maps folder: {folder}"), x, y + 26.0, 15.0, DIM);
                text("Map name:", x, y + 60.0, 17.0, INK);
                text_field("saveas:name", x + 100.0, y + 44.0, r.w - 140.0, 26.0, name, false);
                text("Use \"Save to game folder\" in the toolbar to put it where the game finds it.", x, y + 100.0, 15.0, DIM);
                let go = button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, "Save", true) || (is_key_pressed(KeyCode::Enter) && !popup_open());
                if go {
                    let n = name.clone();
                    self.modal = None;
                    self.save(&n, Destination::UserFolder, Consent::default());
                    return action;
                }
                if cancel(&r) || esc {
                    keep = false;
                }
            }
            Modal::NewMap { size, w: mw, h: mh, fill } => {
                text("New map", x, y, 22.0, ACCENT);
                y += 20.0;
                text("Size", x, y + 18.0, 17.0, INK);
                for (k, n) in MAP_SIZES.iter().enumerate() {
                    if toggle_button(x + 90.0 + k as f32 * 94.0, y, 88.0, 28.0, &format!("{n} x {n}"), *size == k) {
                        *size = k;
                        (*mw, *mh) = (*n, *n);
                    }
                }
                if toggle_button(x + 90.0 + 3.0 * 94.0, y, 88.0, 28.0, "Custom", *size == 3) {
                    *size = 3;
                }
                y += 40.0;
                if *size == 3 {
                    text("Width", x, y + 18.0, 17.0, INK);
                    if let Some(v) = number_field("new:w", x + 90.0, y, 150.0, *mw as i64, 10, 800) {
                        *mw = v as u32;
                    }
                    text("Height", x + 260.0, y + 18.0, 17.0, INK);
                    if let Some(v) = number_field("new:h", x + 330.0, y, 150.0, *mh as i64, 10, 800) {
                        *mh = v as u32;
                    }
                }
                y += 40.0;
                text("Ground", x, y + 18.0, 17.0, INK);
                let surfaces: Vec<(i64, String)> = SURFACE_LABELS.iter().enumerate().map(|(i, l)| (i as i64, l.to_string())).collect();
                if let Some(v) = dropdown("new:fill", x + 90.0, y, 240.0, *fill as i64, &surfaces) {
                    *fill = v as u8;
                }
                if button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, "Create", true) {
                    let o = NewMap { width: *mw, height: *mh, fill: *fill };
                    self.modal = None;
                    return self.guarded(Then::New(o));
                }
                if cancel(&r) || esc {
                    keep = false;
                }
            }
            Modal::Open { path, scroll } => {
                text("Open a map", x, y, 22.0, ACCENT);
                y += 16.0;
                let mut entries: Vec<(String, PathBuf)> = Vec::new();
                for m in self.install_maps(assets) {
                    entries.push((format!("Game: {}", m.name), m.path));
                }
                if let Some(d) = &self.user_dir {
                    for m in files::list_dir(d) {
                        entries.push((format!("Yours: {}", m.name), m.path));
                    }
                }
                let list = Rect::new(x, y, r.w - 40.0, r.h - 190.0);
                let rows = (list.h / 26.0).floor() as usize;
                if mouse_in(list.x, list.y, list.w, list.h) {
                    let wh = mouse_wheel().1;
                    if wh > 0.0 {
                        *scroll = scroll.saturating_sub(2);
                    } else if wh < 0.0 {
                        *scroll += 2;
                    }
                }
                *scroll = (*scroll).min(entries.len().saturating_sub(rows));
                if entries.is_empty() {
                    text("No maps found. Set RAZDOR_DT_DIR for the game's maps, or type a path below.", x, y + 20.0, 16.0, DIM);
                }
                let mut pick = None;
                for (i, (label, p)) in entries.iter().enumerate().skip(*scroll).take(rows) {
                    let ry = y + (i - *scroll) as f32 * 26.0;
                    if small_button(list.x, ry, list.w, 24.0, label, true) {
                        pick = Some(p.clone());
                    }
                }
                let fy = r.bottom() - 110.0;
                text("Or a file:", x, fy + 18.0, 17.0, INK);
                text_field("open:path", x + 90.0, fy, r.w - 130.0, 26.0, path, false);
                if button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, "Open file", !path.trim().is_empty()) {
                    pick = Some(PathBuf::from(path.trim()));
                }
                if let Some(p) = pick {
                    self.modal = None;
                    return self.guarded(Then::Open(p));
                }
                if cancel(&r) || esc {
                    keep = false;
                }
            }
            Modal::Issues { scroll } => {
                let errors = self.issues.iter().filter(|i| i.severity == Severity::Error).count();
                let warnings = self.issues.len() - errors;
                let head = if self.issues.is_empty() { "No problems found.".to_string() } else { format!("{errors} error(s), {warnings} warning(s). Errors block saving. Click one to go there.") };
                text("Map check", x, y, 22.0, ACCENT);
                text(&head, x, y + 26.0, 16.0, if errors > 0 { RED } else { INK });
                y += 40.0;
                let list = Rect::new(x, y, r.w - 40.0, r.h - 130.0);
                let rows = (list.h / 24.0).floor() as usize;
                if mouse_in(list.x, list.y, list.w, list.h) {
                    let wh = mouse_wheel().1;
                    if wh > 0.0 {
                        *scroll = scroll.saturating_sub(2);
                    } else if wh < 0.0 {
                        *scroll += 2;
                    }
                }
                *scroll = (*scroll).min(self.issues.len().saturating_sub(rows));
                let mut go = None;
                for (i, issue) in self.issues.iter().enumerate().skip(*scroll).take(rows) {
                    let ry = y + (i - *scroll) as f32 * 24.0;
                    let mut line = issue.to_string();
                    while measure(&line, 15.0).width > list.w - 10.0 && !line.is_empty() {
                        line.pop();
                    }
                    let hover = mouse_in(list.x, ry, list.w, 22.0);
                    if hover {
                        draw_rectangle(list.x, ry, list.w, 22.0, Color::new(0.3, 0.25, 0.15, 1.0));
                    }
                    text(&line, list.x + 4.0, ry + 16.0, 15.0, if issue.severity == Severity::Error { Color::new(1.0, 0.5, 0.45, 1.0) } else { INK });
                    if hover && clicked() {
                        go = Some(issue.place);
                    }
                }
                if let Some(p) = go {
                    self.go_to(p);
                    return action;
                }
                if button(r.right() - 140.0, r.bottom() - 54.0, 120.0, 40.0, "Close", true) || esc {
                    keep = false;
                }
            }
            Modal::Settings => unreachable!("handled above"),
        }
        if keep {
            self.modal = Some(next);
        }
        action = EditorAction::None;
        action
    }
}
