# Diff test

## Razdor side (script mode)

Razdor plays an action list (v1) without a window and writes the state (schema v1) after
every action:

    cargo build --release
    target/release/razdor --replay actions.jsonl --out out/        # list starts with new_game
    target/release/razdor --replay actions.jsonl --map РК1 --hero 1 --out out/

The install comes from `RAZDOR_DT_DIR` (or `.env`); a list on `"map":"demo"` needs none.
`--map` puts a `new_game` before the list (file name with or without `.DTm`, or a unique
prefix). States go to `out/razdor.jsonl`, one line per action (`step` = the action's index,
0-based), or to the standard output without `--out`. Actions that do not apply at that
moment (an `ok` with nothing open, a click on a cell that is no target) are skipped and
listed on stderr as `note:` lines.

How the actions are applied (`src/difftest.rs`):
- `click_map`: both clicks of the original at once; the walk plays to its end, or until a
  message, a fight or a building window stops it. An open building window is closed first.
- `wait`: 1 or 4 hours of 30-minute ticks; a message or the noon report stops it.
- `ok` closes the front dialog, else the building window; `answer` the front question.
- `battle_auto`: the battle AI plays both sides to the end and the result box is closed.
- `key`: `Escape`, `1`, `4`, `Return`/`space`; others are noted and skipped.
- The generator gets the interface's draws: `Random(3)` as an event, village or shipyard
  window opens, and the music change when the first dialog after a won battle closes. The
  map music's real-time rotation is not played (it has no fixed place in a script).

Field meanings follow `memread.py`: `clock` = time div 100 + start offset; unit `type`
1-based, `hp` the hit points (max when unhurt); building `owner` 0 player, k army, 255
none; `goods` the non-empty goods without sign (ruins: their treasure); `events_done` the
events fired at least once. An army Razdor dropped after its defeat (no respawn) is given
as `{"id", "alive": false}` only; an army waiting to respawn as `active` and `alive` false.
