//! Dragging a unit's card to another cell of the army grid (the army screen, the barracks):
//! pressed on a card, it follows the mouse once moved; let go over another cell, the unit
//! goes there, swapping with a unit standing in it (`Game::move_unit`). A press that does
//! not move stays a click.

use std::cell::Cell;

use macroquad::prelude::*;
use razdor::rules::battle::Team;
use razdor::rules::content::UnitId;
use razdor::rules::formation::Slot;

use super::assets::Assets;
use super::widgets::{input_blocked, pointer};

/// Pixels the mouse must move before a press on a card drags it.
const START: f32 = 5.0;

#[derive(Clone, Copy)]
struct Drag {
    /// Squad index, and its portrait.
    unit: usize,
    kind: UnitId,
    start: Vec2,
    moved: bool,
}

thread_local! {
    static DRAG: Cell<Option<Drag>> = const { Cell::new(None) };
}

/// A press on squad member `unit`'s card: it may be dragged from here.
pub fn press(unit: usize, kind: UnitId) {
    DRAG.with(|d| d.set(Some(Drag { unit, kind, start: pointer().into(), moved: false })));
}

/// The unit being dragged, once the mouse has moved (its card is drawn dimmed).
pub fn dragged() -> Option<usize> {
    DRAG.with(|d| d.get()).filter(|d| d.moved).map(|d| d.unit)
}

/// After the grid is drawn: follows the mouse with the portrait (card `size`), and on the
/// release over one of `cells` other than the unit's own returns (unit, cell).
pub fn update(assets: &Assets, cells: &[(Slot, Rect)], size: Vec2) -> Option<(usize, Slot)> {
    let mut d = DRAG.with(|d| d.get())?;
    let m = Vec2::from(pointer());
    if !d.moved && m.distance(d.start) > START {
        d.moved = true;
    }
    if is_mouse_button_down(MouseButton::Left) && !input_blocked() {
        if d.moved {
            let r = Rect::new(m.x - size.x / 2.0, m.y - size.x / 2.0, size.x, size.x);
            assets.draw_portrait(d.kind, Team::Player, r);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, super::chrome::GOLD);
        }
        DRAG.with(|c| c.set(Some(d)));
        return None;
    }
    DRAG.with(|c| c.set(None));
    if !d.moved {
        return None;
    }
    cells.iter().find(|(_, r)| r.contains(m)).map(|&(slot, _)| (d.unit, slot))
}

/// Leaving the screen: nothing stays held.
pub fn cancel() {
    DRAG.with(|d| d.set(None));
}
