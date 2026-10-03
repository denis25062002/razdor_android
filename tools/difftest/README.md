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

## Original side (Wine + memory reader)

`original.py` plays an action list in the original Discord Times and writes the same state
schema, read from the game's memory by `memread.py`:

    ~/.local/opt/re-venv/bin/python -m tools.difftest.original --actions actions.jsonl --out out/
    ~/.local/opt/re-venv/bin/python -m tools.difftest.original --map РК1 --hero 1 --actions a.jsonl --out out/
    ~/.local/opt/re-venv/bin/python -m tools.difftest.original --map РК1 --out out/ --check

Output: `out/state-NNNN.json` and `out/shot-NNNN.png` per action (`NNNN` = the action's
index), the same states one per line in `out/original.jsonl`, and `out/run.jsonl` with each
action, a skip note when it did not apply, and facts outside the schema (screen, camera,
game time in centi-minutes, the music timer). `--check` stops after the new game and
compares memory with the map file (exit 1 on a mismatch).

**Setup.** Needs `Xvfb`, Wine (tested with 11.0) with a 32-bit prefix (`~/.wine`, or
`--wineprefix`), the `ru_RU.UTF-8` locale, and the RE venv (`python-xlib`, `pillow`). The
install (`--install`, default `~/Games/Discord Times Community Update`) is only read: the
game runs from a copy in `--work` (default `~/.cache/razdor-difftest/install`, made once;
its ini files are refreshed from the install on every run) with `[Tutorial] Completed=1`, so
New game opens the scenario list. The game writes its caches, autosaves and logs there.

**What it starts and stops.** Xvfb on the first free display from `:77` (never the
desktop), then `wine DiscordTimes.exe` with `LANG`/`LC_ALL=ru_RU.UTF-8` (Cyrillic map names
crash the loader otherwise), `WINEDLLOVERRIDES=winepulse.drv=d` and an ALSA config whose
default device is `null` (the game refuses to start without a sound driver; this one is
silent). At the end it stops the game, Xvfb, and the wineserver if none was running before.
A start that hangs before the main menu (seen once) is retried once.

**Actions.**
- `new_game`: main menu → New game → the map's row in the scenario list → Next → the hero
  class portrait (`hero` 1 knight, 2 archmage, 3 ranger) → Start. Only standalone maps and
  the first map of a campaign are in the list (`kind` 2 maps cannot be started), and only
  from the main menu, so a list has one `new_game`, first. Map names as for Razdor: the file
  name with or without `.DTm`, or a unique prefix.
- `click_map`: a building window still open is closed (Esc); the cell becomes a pixel through
  the camera read from memory (`px = (x+1)·32 + 16 − camera x`, `py = (y+1)·22 + 11 −
  camera y`), scrolling with the arrow keys while it is outside the safe part of the view;
  the first click plans the route, and when the planner reached the cell a second click sets
  off, as Razdor's click does.
- `wait`: hover the message box, then click its left (1 h) or right (4 h) button.
- `ok`: the event window's OK button (its rectangle read from memory), else Esc for a
  building, village or shipyard window, else Return.
- `answer`: the event window's Yes or No button (from memory); Return/Esc on the yes/no box.
- `key`: an X keysym name (`Escape`, `Return`, `space`, `F4`, ...).
- `battle_auto`: the original has no auto battle; the step is recorded with a note.
- `snapshot`: only records.

After each action the harness waits until the game is at rest: on the world map with the
idle flag set and the timeline queue empty, or on another window, and nothing (screen,
time, generator, hero cell) changed for 0.6 s. An action that does not apply (wrong screen,
no question) is skipped with a note, like Razdor's.

**State fields** (addresses in `memread.py`):

| field | source | checked against |
|---|---|---|
| `clock` | time 0x68dcb8 (centi-minutes) div 100 + start 0x68dcbc (= header start + 1) | the date on the bar; header start + 1 at load |
| `rng` | 0x659154 | 24 LCG steps from 1 after loading РК1 (market restocks + music) |
| `hero.x/y` | hero army +0x1724/+0x1728 | preset start, the cell table 0x75a544, clicks |
| `hero.gold/mana` | 0x75c018 / 0x68e4f4 | preset; the bar after a village tribute |
| `units` | army +4 + i·0x1db: type +0 (written 1-based), level +0x10 (0-based, as in the file), hp +0x20 (−1 = unhurt, written as max HP +0xde), xp +4 | preset troops, map armies, hero HP on the class screen |
| `armies` | slots 1..N (0x68ecd4) of 0x75a940 + k·0x3827: x/y +0x1724/+0x1728, `active` = on map +0x16a1, `alive` = not destroyed +0x16a2, gold +0x16d8 | file positions, gold, inactive flag, units |
| `buildings` | [0x68ece0] + (b−1)·0x166: owner +0x124 (0 player, k army, 255 none), gold +0x11e, mana +0x160, goods = non-zero words at +0x88 without sign | owner and start buildings, village stock, market goods; tribute taken |
| `events_done` | events whose times-fired counter (+0xa0) is above 0 | the start event counts once its OK is pressed |

`--check` passes on РК1, Тихая пристань, Проклятое озеро, Устье Трейна, Другой берег and
ДС1 (all checks; events fired during the load are taken into account: their results are
applied before their dialog is closed, their counter only when it closes).

Not validated by a live change yet: `xp` (0 at start; no event of РК1 gives XP and the
original has no auto battle), unit `hp` and `level` after a battle, army `gold` after
the AI earns or spends.

**Things to know when diffing.**
- The original changes the music on a real-time timer (50-90 s) and that change draws the
  generator (engine.md §3.4). A run that idles long enough gets an extra draw;
  `run.jsonl` has the timer (`next_music_ms`) and the clock (`now_ms`) to spot it.
- An event's results apply when it fires; `events_done` lists it only after its window
  closes.
- `click_map` on a cell that cannot be entered (wood, water) does nothing in the original.

**Pixel layout at 1024×768** (for the harness and for anyone driving the game by hand):
main menu New game (512, 248); scenario list rows at x = 330 from y = 262 (one row per
listed title; every row of a campaign selects its first map; the list selection is
0x65adab = map-list entry), scroll-down arrow (497, 634), Next (665, 688); hero window
portraits (315 / 512 / 709, 325), Start (665, 688); event window OK 96×36 centred under the
text (e.g. (512, 461) for the start message of РК1), Yes on the left and No on the right
(e.g. (348, 476) and (674, 476)); bottom panel: message box (372, 684) 280×60, its wait
buttons appear on hover at about (418, 713) 1 h and (607, 713) 4 h, the centre button
(512, 713) glides to the hero; resource line at y = 756: mana, gold, income, upkeep.
