//! Typed edits of a scenario. Every change the editor makes is one of these, applied by
//! [`super::EditorDoc::apply`] so it can be undone.

use crate::dt::dtm::{Army, Building, Event, Header, NamedCharacter, Point};

use super::doc::Target;

/// Which objects an erase removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectFilter {
    All,
    /// Hills, mountains, stones (classes 1–8).
    Massifs,
    /// Trees and thickets (classes 9–12).
    Plants,
}

/// The scenario settings edited together: the header (minus the size, which never changes
/// here) and the scenario's own strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub header: Header,
    pub title: String,
    pub description: String,
    pub campaign_name: String,
    pub next_map: String,
    pub named_characters: Vec<NamedCharacter>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// A square brush of terrain `code` centred on the cell.
    PaintTerrain { x: i32, y: i32, size: u32, code: u8 },
    /// The 4-connected region of the cell's surface becomes `code`.
    FillTerrain { x: u32, y: u32, code: u8 },
    /// A rectangle of `code` between two corners.
    RectTerrain { from: (i32, i32), to: (i32, i32), code: u8 },
    /// One object on every cell of the brush (a cell holding this very object is skipped).
    PlaceObjects { x: i32, y: i32, size: u32, class: u8, sprite: u8 },
    /// Every object (of the filter) standing on a cell of the brush.
    EraseObjects { x: i32, y: i32, size: u32, filter: ObjectFilter },
    /// A new building with the defaults of its type; `(x, y)` is the bottom-right cell.
    PlaceBuilding { x: u16, y: u16, kind: u8, picture_type: u8, variant: u8, size: (u8, u8) },
    MoveBuilding { id: u16, x: u16, y: u16 },
    /// Removes a building; later ids shift down and references follow ([`super::refs`]).
    DeleteBuilding { id: u16 },
    /// Replaces a building's record (its property panel).
    SetBuilding { id: u16, building: Box<Building> },
    /// A new army with the defaults.
    PlaceArmy { x: u16, y: u16 },
    MoveArmy { id: u8, x: u16, y: u16 },
    DeleteArmy { id: u8 },
    SetArmy { id: u8, army: Box<Army> },
    /// A new lantern (lit, radius 5) or event point.
    PlacePoint { x: u16, y: u16, lantern: bool },
    MovePoint { id: u8, x: u16, y: u16 },
    DeletePoint { id: u8 },
    SetPoint { id: u8, point: Box<Point> },
    /// Replaces the scenario settings.
    SetSettings(Box<Settings>),
    /// A named character at the end of the list.
    AddNamedCharacter { unit: u8, name: String },
    /// Removes named character `index` (1-based); armies and events follow.
    RemoveNamedCharacter { index: u8 },
    /// A new event of type `kind` (1 global … 4 rumour) at the end of the list.
    NewEvent { kind: u8 },
    /// A copy of event `id` (its picture included) at the end of the list.
    DuplicateEvent { id: u16 },
    /// Removes an event; later ids shift down and every reference follows
    /// ([`super::refs::remove_event`]).
    DeleteEvent { id: u16 },
    /// Replaces an event's record (its property panel).
    SetEvent { id: u16, event: Box<Event> },
    /// Adds event `event` to a building's or point's local list.
    AttachEvent { place: Target, event: u16 },
    /// Takes event `event` out of a building's or point's list.
    DetachEvent { place: Target, event: u16 },
}

impl Command {
    /// A short label for the undo list.
    pub fn label(&self) -> &'static str {
        match self {
            Command::PaintTerrain { .. } => "Paint terrain",
            Command::FillTerrain { .. } => "Fill terrain",
            Command::RectTerrain { .. } => "Terrain rectangle",
            Command::PlaceObjects { .. } => "Place objects",
            Command::EraseObjects { .. } => "Erase objects",
            Command::PlaceBuilding { .. } => "Place building",
            Command::MoveBuilding { .. } => "Move building",
            Command::DeleteBuilding { .. } => "Delete building",
            Command::SetBuilding { .. } => "Edit building",
            Command::PlaceArmy { .. } => "Place army",
            Command::MoveArmy { .. } => "Move army",
            Command::DeleteArmy { .. } => "Delete army",
            Command::SetArmy { .. } => "Edit army",
            Command::PlacePoint { .. } => "Place point",
            Command::MovePoint { .. } => "Move point",
            Command::DeletePoint { .. } => "Delete point",
            Command::SetPoint { .. } => "Edit point",
            Command::SetSettings(_) => "Edit scenario settings",
            Command::AddNamedCharacter { .. } => "Add named character",
            Command::RemoveNamedCharacter { .. } => "Remove named character",
            Command::NewEvent { .. } => "New event",
            Command::DuplicateEvent { .. } => "Duplicate event",
            Command::DeleteEvent { .. } => "Delete event",
            Command::SetEvent { .. } => "Edit event",
            Command::AttachEvent { .. } => "Attach event",
            Command::DetachEvent { .. } => "Detach event",
        }
    }

    /// The parts of the scenario the command may change (for undo snapshots).
    pub(super) fn sections(&self) -> Sections {
        use Sections as S;
        match self {
            Command::PaintTerrain { .. } | Command::FillTerrain { .. } | Command::RectTerrain { .. } => S::TERRAIN,
            Command::PlaceObjects { .. } | Command::EraseObjects { .. } => S::OBJECTS,
            Command::PlaceBuilding { .. } | Command::MoveBuilding { .. } | Command::SetBuilding { .. } => S::BUILDINGS,
            Command::DeleteBuilding { .. } => S::BUILDINGS | S::ARMIES | S::EVENTS | S::META,
            Command::PlaceArmy { .. } | Command::MoveArmy { .. } | Command::SetArmy { .. } => S::ARMIES,
            Command::DeleteArmy { .. } => S::ARMIES | S::BUILDINGS | S::EVENTS,
            Command::PlacePoint { .. } | Command::MovePoint { .. } | Command::SetPoint { .. } => S::POINTS,
            Command::DeletePoint { .. } => S::POINTS | S::EVENTS,
            Command::SetSettings(_) | Command::AddNamedCharacter { .. } => S::META,
            Command::RemoveNamedCharacter { .. } => S::META | S::ARMIES | S::EVENTS,
            Command::NewEvent { .. } | Command::DuplicateEvent { .. } | Command::SetEvent { .. } => S::EVENTS,
            Command::DeleteEvent { .. } => S::EVENTS | S::BUILDINGS | S::POINTS | S::META,
            Command::AttachEvent { place, .. } | Command::DetachEvent { place, .. } => match place {
                Target::Building(_) => S::BUILDINGS,
                Target::Point(_) => S::POINTS,
                // Armies hold no event lists; the command fails.
                Target::Army(_) => S::EVENTS,
            },
        }
    }
}

/// A set of scenario parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sections(pub(super) u8);

impl Sections {
    pub const TERRAIN: Sections = Sections(1);
    pub const OBJECTS: Sections = Sections(2);
    pub const BUILDINGS: Sections = Sections(4);
    pub const ARMIES: Sections = Sections(8);
    pub const POINTS: Sections = Sections(16);
    pub const EVENTS: Sections = Sections(32);
    /// Header and scenario strings.
    pub const META: Sections = Sections(64);
    pub const ALL: [Sections; 7] =
        [Sections::TERRAIN, Sections::OBJECTS, Sections::BUILDINGS, Sections::ARMIES, Sections::POINTS, Sections::EVENTS, Sections::META];

    pub fn contains(self, o: Sections) -> bool {
        self.0 & o.0 == o.0
    }
}

impl std::ops::BitOr for Sections {
    type Output = Sections;
    fn bitor(self, o: Sections) -> Sections {
        Sections(self.0 | o.0)
    }
}
