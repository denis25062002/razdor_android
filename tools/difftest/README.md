# Diff test

Play the same actions in the original Discord Times and in Razdor and compare their game
state after every step. The whole pipeline is one command:

    ~/.local/opt/re-venv/bin/python -m tools.difftest.run --actions rk1.jsonl --map РК1 --hero 1

It builds Razdor (release, without sound, into `~/.cache/razdor-difftest/target`), plays the
list in Razdor (`razdor --replay`), in the original (`python -m tools.difftest.original`, under
Wine on a hidden Xvfb display), and once more in Razdor with each step starting from the
original's generator state of the step before (`--rng-from`), then writes
`~/.cache/razdor-difftest/runs/<name>/report.md` (`--name`, default the list's name and the
time) and `diff.json`. Exit code 0 when every step compares equal, 1 otherwise.

**The report.** The first step and the fields where the free run differs, with both values,
the explanation of a generator mismatch and the screenshots of both sides at that step
(`original/shot-NNNN.png`; Razdor's `razdor-shot-NNNN.png` through `RAZDOR_SCENE=replay`, on
its own hidden display); then a table per step for the free run and for the step-local run
(a generator difference in one step does not spill into the next ones there); then the fields
only one side has.

**How it compares.** Only the fields both sides have (a missing one is listed, not counted).
Armies by id, other lists by position. Two differences of timing are taken into account:
- the original counts an event as done when its window closes, Razdor when it fires: the
  event the original shows is counted as done (not the engine's own reports, which use the
  slot after the last event);
- a difference seen while the original shows a window (event, village, building, shipyard,
  question) that is gone once the window is closed with only closing actions is reported as
  *window timing*, not as a divergence (the original applies an event's finishing results at
  OK and pays a village's stock when its window closes; Razdor does both at once).

**Generator mismatches** are explained three ways: by stepping the generator ("the original is
k draws ahead"), by the two draw traces (each side's `Random(n)` of the step with its source:
Razdor's file and line, the original's return address and the caller's name from engine.md
§3.4; the first draw where the `n` differ is shown), and by the music: the original's timed
music change is held off in these runs (`--real-music` lets it draw, and it is then flagged
as real-time noise).

`rk1-day1.jsonl` is the list of the first run: РК1 with the knight, the first day (the
village, noon, the opening events, the waits through midnight) and the ruins' garrison
fought by explicit actions (planned against the original, so Razdor notes the presses its
own course of the battle has no use for; `battle_auto` then ends Razdor's battle if it is
still on, and is a no-op in the original).

Options: `--no-build`, `--exe PATH` (this Razdor binary, no build), `--reuse-original DIR`
(take the original's side of an earlier run), `--real-music`, `--no-trace`, `--no-shots`,
`--trace PRESETS` (the Frida runtime trace below), `--av` (sounds and animations, below). `FINDINGS.md` lists the differences found so far.

## Runtime trace (Frida)

`trace.py` hooks any function of the original by address and logs each call (arguments,
memory read before and after, return value) as JSON lines tagged with the step:

    python -m tools.difftest.trace --list                       # the presets
    python -m tools.difftest.run --actions a.jsonl --trace random,ai:1,events
    python -m tools.difftest.original --actions a.jsonl --out out/ --trace ai_frames:1,my-hooks.json
    python -m tools.difftest.trace --read out/trace.jsonl --steps 8-10 --army 1

Records go to `original/trace.jsonl` (`s` step, `q` sequence, `f` hook, `t`/`tl` game time
at entry/return, `ra` return address, `a` arguments, `e`/`l` values read at entry/return,
`r` return value; step −1 is before the first action). Presets:

| preset | hooks |
|---|---|
| `random` | Random 0x4832fc: `n` (EAX), the state before, the result. With it Frida gives `run.jsonl`'s `draws` and the stub is not installed |
| `ai[:K]` | the step clock 0x4a399c (step starts and arrivals: cell, direction, play time, bank, window, path index), goal choice 0x4a2d88 (the new path), wander points 0x4a2550, arrival rules 0x4a548c, the snap at the hero's stop 0x4ad8a0; `:K` = army K only |
| `ai_frames[:K]` | every call of the step clock (each frame of game time) |
| `advance` | World_AdvanceAI 0x4ade3c (the per-frame AI driver, `dt`) |
| `damage` | physical damage 0x485908 (kind, both units' type and HP, the result) and ApplyDamage 0x48a354 (HP before/after) |
| `events` | the event scan 0x4abfbc, opening an event 0x4a8ae8 (0-based index), Event_Finish 0x4ab1ec |
| `av` | Sound_Play 0x481420 (slot, restart, loop; the slot's group and `_Sounds.ini` key), Music_Play 0x49d774, Music_NextRandom 0x49d7f8, and every entry pushed on the deferred-call queues (0x48c2e4, 0x48c348): the callback and its {a, b, c}. See "Sounds and animations" |

A JSON file in the list adds hook specs of its own (format in `trace_agent.js`: address,
arguments by register or stack slot in Delphi's register convention, JavaScript expressions
over the arguments and memory for the reads and the filters). The expressions run inside the
game: only use hook files you wrote.

**Setup.** `pip install frida` (tested with 17.22.0) in the venv
that runs the harness, and the matching Windows x86 gadget from the Frida releases
(`frida-gadget-<version>-windows-x86.dll.xz`, unpacked into `~/.local/opt/frida-win/`, or
`--gadget PATH` / `RAZDOR_FRIDA_GADGET`).

**How it gets in.** Wine 11 runs the game through new-style WoW64: 32-bit code in a 64-bit
Linux process (`wine-preloader`). A Linux Frida cannot attach to it: its 64-bit agent is
injected but aborts ("Unable to locate the libc": the main image is the static preloader)
and the game dies with SIGSEGV; and a 64-bit agent could not hook the 32-bit code anyway.
So the Windows x86 gadget runs inside the game: the harness copies it into the private
install copy with a config that listens on a free 127.0.0.1 port, writes a one-shot stub
into unused space of `.mod` (0xc2e000) and points the import slot of `timeGetTime`
(0xc0b834, called every frame) at it; the stub restores the slot, calls
`LoadLibraryA("frida-gadget.dll")` and goes on into `timeGetTime`. The gadget loads within a
frame at the main menu; the harness then connects to it and loads `trace_agent.js`. No
proxy DLL, no registry or prefix change, nothing in the real install. Frida's frida-server
for Windows was not needed and not tried.

**Reliability.** On РК1 (the first 14 steps of `rk1-day1.jsonl`) the `random` hook gave the
very draws the stub gave, step by step, and also the 2721 map-load draws the stub's ring
loses; the agent buffers up to 500000 records between two steps and counts any it drops
(`trace_dropped` in `run.jsonl`). Hooks cost time in the game's frames, and the original's AI
movement depends on the frame rate (FINDINGS.md §5): the presets above did not change the
game in repeated runs, but a slow hook (25 ms per frame) moved an army three cells less.

## Sounds and animations (`--av`, `av.py`)

    python -m tools.difftest.run --actions tools/difftest/rk1-day1.jsonl --av
    python -m tools.difftest.av --run RUN_DIR --events       # both sides' events, step by step
    python -m tools.difftest.av --coverage RUN_DIR...        # the table of AV.md

`run.py --av` adds the Frida preset `av` to the original's trace and an "Audio and effects"
section to `report.md` (and `av.json`): per step, the sounds (`sfx`, by their
`[SFX-Effects]` key), the music tracks started (`music`, `[Backgrounds]` key) and the
animations started (`anim`) of the original against Razdor's step-local run (the chords and
the music picks are the generator's draws). Compared per kind: the names as a multiset
(missing in Razdor, extra in Razdor), then the order of the common ones; battle effects and
slides also by their card (`side:row:col`, side 1 the player's) when both sides have as many.

The original: a sound is a Sound_Play of a slot of group 2 (the music's own plays, group 1,
are left out; Music_Play and Music_NextRandom name the track); the slot is named by the
globals the loader fills (0x4e2e80, `trace.SOUND_GLOBALS`). An animation is an entry pushed
on a deferred-call queue, which is how the game starts every timed thing (`av.QUEUE_FNS`):
battle slide 0x4afbd8 (b = from place | side << 8 | to place << 16 | side << 24), effect
0x4afe7c (b = place | side << 8 | sound << 16, c = picture: 0 shot, 1 melee, 2 magic,
3 bless, 4 cure), pass 0x4afb54, card slide 0x4b0284, the won battle's hold 0x4b09e8, walk
0x4ae6dc, wait 0x4ae280, camera glide 0x4af96c (c = cell x | y << 16), reveal 0x4af83c,
look at an army 0x4afa98, world spell 0x4af2f8, a unit moved or hired 0x4b0c04, a unit's
heal or potion 0x4b11cc, promotion 0x4b1a04; the event chain 0x4af658 and the class
portrait 0x4b2044 only sequence things and are not compared.

Razdor: the replay logs (`razdor-run.jsonl`, `av` per step; `src/av.rs`) what the
interface would cue at the same points, without a window: the window sounds of
`App::sounds` (an event, village or shipyard chord, `InterfaceCastSpell` for a building
window, `Global-Battle`), the buttons the ops stand for (dialog OK/Yes/No, the wait button,
the village window's close with the tribute's `Item-Gold`, the panel icon before the army
window or the book, the tab presses the harness makes to reach a service's tab, the market's
list switch, the `Item-Gold` of the money buttons; the map clicks are silent), the new
game's menu bell and presses, the fog opening at the map start, the battle's sounds and
effects (`av::BattleSound`, shared with `battle_view`; the actor's lunge is logged as
`battle_slide`, a counterblow's lunge back and effect after it, `av::echo`), a pass's
pause, the won battle's hold, the hired card's slide, the cure in a building, the camera to
a spell's army and the spell's effect, item sounds, the music (map start, battle themes,
triumph, the track after the victory box, defeat) and the walk, the wait and the flights to
the places an event shows, after its window. `AV.md` has the coverage over the runs so far.

## Battles (action list and state, v1 extension)

    {"op":"battle_act","side":2,"row":1,"col":4}   a press on that card (side 1 own, 2 enemy)
    {"op":"battle_pass"}                            the space key

Rows 1 front, 2 back, 3 reserve; columns 1-6 as the original's grid numbers them (6-column
formation; in it the back row has columns 2-5 and the reserve 3-4). A press does what the
cell holds for the unit whose turn it is: a strike, a shot or a spell on an enemy, a heal or
a blessing on a friend, a pass on its own card, a step to an empty own cell. The enemy's turns
then play until the player's next turn or the end. A press with no action is noted.

While a battle is on screen the state has
`"battle": {"turn", "actor": [side, row, col], "sides": [[unit...], [unit...]]}`, a unit being
`{"type", "row", "col", "hp", "actions"}` in the side's record order (a dead unit's record is
removed); `actor` only while the player has the input. Original: the battle object 0x668cf8
(`memread.Game.battle`), cards by Formation_CellToSlot (0x492940): front row places 0-5,
back row 7-10, reserve 6 and 11 (`memread.WIDE_PLACES`), the card widgets at 0x66aea0 (enemy)
and 0x66b224 (own) + place × 0x4b. The run log also gets the units' attack, defence and
initiative fields (`battle_raw`) on both sides.

## Services (action list and state, v1 extension)

    {"op":"buy","slot":n}   {"op":"sell","slot":n}     the market tab: row n of the goods / of the sell list
    {"op":"hire","slot":n}                             the hire tab: barracks slot n (the building record's)
    {"op":"heal","unit":i}  {"op":"resurrect","unit":i}  the hire tab: unit i of the hero's army
    {"op":"learn","slot":n}                            the sanctuary tab: row n of its spells
    {"op":"cast","slot":k[,"army":id]}                 on the map: book entry k (on army id for a spell on enemies)
    {"op":"equip","slot":n,"unit":i}                   the army window: pack item n on unit i

Rows, slots and units count from 0; a pack item's number counts the pack's items in pack order,
empty places left out. The building ops need the building window open (the hero stands in
the building); `cast` and `equip` close it first, as opening a side window does in the
original. A village's offer is a yes/no question in the original's event window: `answer`
takes it (Razdor asks it the same way; no gives the village window). There is no hero spell
in battle in the original (the book is a world window), so `cast` is a map action only.

The state's hero gets `pack` (item numbers in pack order, empty places left out: 0x68dce0),
`book` (the spell book: 0x68e0e8, count 0x68e4e8) and per unit `items` (the four worn slots,
0 empty: unit +0xcd).

How the original's side presses them (`original.py`; widgets found by listing the screen's
child widgets, each with its rectangle at +0x11.. and its enabled flag at +0xd): a tab is
chosen by pressing the tab buttons from the top until the window's tab (0x68dc88: 0 hall, 1
hire, 2 garrison, 3 market, 4 sanctuary) is the one wanted; the market list (widget 0x670688,
first shown row at +0x6b, 7 rows of 28 px) is scrolled by a long press near the end of its
scroll bar, the row is clicked and checked against the selected row (0x671258 goods, 0x671254
sell list), then Buy/Sell (0x6709bc) is pressed; the list switch buttons are "Inventory"
(0x670b30, the sell list) and "Trade shop" (0x670ca4, the goods). Hire: the six buttons
0x66efb0 + k·0x171 by barracks slot; heal and resurrect: the twelve buttons 0x66de64 +
p·0x171 by the unit's card place p (its cell in the army grid at army +0x1630 + r·0x18 + c·4,
then the place table of the battle cards). Learn: the spell list 0x671334 (6 rows) and the
same button. Cast: the panel's book button, the cell 0x66c2fc + k·0x4b; a spell on enemies
then waits for a click on the target army's cell. Equip: the panel's army button, the pack
cell (5×5 from (338, 58), 55 px), then for the hero the lowest empty worn slot of the window,
for another unit its card (0x667f3c + place·0x4b). Each op checks that something changed
(pack, gold, army, book, worn items) and notes it when nothing did. Checked live on ДС1
(hire, buy, sell, learn, equip: every step equal), РК1 (heal after the ruins' battle) and
Проклятое озеро (two casts: clock and generator equal).

## Razdor side (script mode)

Razdor plays an action list (v1) without a window and writes the state (schema v1) after
every action:

    cargo build --release
    target/release/razdor --replay actions.jsonl --out out/        # list starts with new_game
    target/release/razdor --replay actions.jsonl --map РК1 --hero 1 --out out/

The install comes from `RAZDOR_DT_DIR` (or `.env`); a list on `"map":"demo"` needs none.
`--rng-from original.jsonl` sets the generator before each step to the state that file
has for the step before. `--look` adds to each state line a `look` object (not part of the
schema): Razdor's screen (`map`, `building`, `dialog`, `question`, `offer`, `battle`,
`ended`), the book and the pack, and in a building window its tabs, goods, sell list,
barracks, heal/raise prices and spells with the row numbers the service ops take; also the
income, the wages and the event whose window is shown.

**Later campaign maps.** `new_game` starts any map of the install, a later campaign map too
(the original's New game only starts a campaign at its first map). With `carry` it starts as
after the map before, through Razdor's own campaign hand-over (`Game::from_campaign`), so
the opening events see it:

    {"op":"new_game","map":"РК3","hero":2,"carry":{"gold":3154,"mana":1172,"hero_level":4,
     "units":[[14,3],[28,3]],"pack":[93],"book":[1,11],"flags":["Band","King"],"reveal":true}}

`units` are `[type, level]` (the state's encodings), `hero_level` 0-based (with it `book`
replaces the map's), `flags` the campaign flags of the earlier maps' event title scripts,
`reveal` the whole map explored. A field left out keeps the map's preset. Used for the
gameplay-video experiment (`VIDEO.md`, `rk3-video.jsonl`); Razdor only. `--map` puts a `new_game` before the list (file name with or without `.DTm`, or a unique
prefix). States go to `out/razdor.jsonl`, one line per action (`step` = the action's index,
0-based), or to the standard output without `--out`. Actions that do not apply at that
moment (an `ok` with nothing open, a click on a cell that is no target) are skipped and
listed on stderr as `note: step N:` lines. With `--out`, `razdor-run.jsonl` has per step
the notes, the generator's draws (`[n, state before, "file:line"]`) and the battle units'
stats. `RAZDOR_SCENE=replay:<step>` with `RAZDOR_REPLAY=<list>` shows the screen after that
step (`src/ui/snapshot.rs`).

How the actions are applied (`src/difftest.rs`):
- `click_map`: both clicks of the original at once; the walk plays to its end, or until a
  message, a fight or a building window stops it. An open building window is closed first.
- `wait`: 1 or 4 hours of 30-minute ticks; a message or the noon report stops it.
- `ok` closes the front dialog, else the building window; `answer` the front question.
- `battle_auto`: the battle AI plays both sides to the end and the result box is closed.
- `battle_act`, `battle_pass`: see "Battles" above.
- `key`: `Escape`, `1`, `4`, `Return`/`space`; others are noted and skipped.
- services (`buy`, `sell`, `hire`, `heal`, `resurrect`, `learn`, `cast`, `equip`): see
  "Services" above; a refusal (no money, no such row) is a note.
- The generator gets the interface's draws: `Random(3)` as an event, village or shipyard
  window opens, and the music change when the first dialog after a won battle closes. The
  map music's real-time rotation is not played (it has no fixed place in a script).

Field meanings follow `memread.py`: `clock` = time div 100 + start offset; unit `type`
1-based, `level` 0-based as the map file and the unit record number it, `hp` the hit points (max when unhurt); building `owner` 0 player, k army, 255
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
- `battle_act`: a click on the card's widget (rectangle read from memory); `battle_pass`:
  Space. Both need the player's turn (input flag 0x68dc63).
- `snapshot`: only records.

After each action the harness waits until the game is at rest: on the world map with the
idle flag set and the timeline queue empty, in battle with the player's input on, or on
another window, and nothing (screen, time, generator, hero cell, battle sides) changed for
0.6 s.

**Draw trace and music** (`--trace-draws`, `--hold-music`; `run.py` turns both on). The
trace writes a 6-byte jump at the top of Random (0x4832fc) into a 76-byte stub placed in
unused, zero space at the end of the Community's `.mod` section (0xc2b000, RWX, no references
to it in the exe); the stub stores the return address, `n`, the state before and the game
time into a 512-slot ring (0xc2b200) and counts the calls (0xc2b100), then runs the displaced
prologue. The harness reads the ring as it waits; `run.jsonl` gets each step's `draws`
(`[n, state before, return address, caller, time_cs]`) and `draws_lost` (the ring wrapped:
the map load's plant jitter always does). `--hold-music` keeps the next timed music change
(0xae123c) an hour ahead of the frame clock (0x4f1c34), so the music rotation, a real-time
draw, never fires during a run; the music changes the game starts itself (a battle, the
triumph's end) still draw. Nothing else is written into the process. An action that does not apply (wrong screen,
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

**Things to know when diffing.** `run.jsonl`'s `meta` also has `event_count` and, in battle,
`battle_raw`.
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

## LLM explorer (`explore.py`)

A local model plays episodes and the diff test looks for divergences in them:

    ~/.local/opt/re-venv/bin/python -m tools.difftest.explore --hours 2 --max-new 3
    ~/.local/opt/re-venv/bin/python -m tools.difftest.explore --episodes 1 --maps РК1 --len 30
    ~/.local/opt/re-venv/bin/python -m tools.difftest.explore --episodes 3 --play-only   # no diff

Needs Ollama (`--ollama`, default `http://localhost:11434`, model `--model huggingface.co/unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF:latest`, the winner of a benchmark on 2026-10-04: same seeded episodes, 1 % invalid actions and 3.1 s a call fully on the GPUs, against 13 % and 4.3 s for qwen3.6, 9 % for gemma4:26b, 50 % for gemma4 and no usable JSON from qwen3-vl:8b;
it is asked a few times and the run gives up if it does not answer). Per episode: a map New
game can start (standalone maps and campaign first maps, from the install's `Maps_Rus`) and
the next hero class (each episode the next one, so with 7 maps every map meets every class
within 21 episodes), an episode length (`--len`, default 40-80 actions) and goals from a
rotating list (every building type, accept and decline offers, friendly armies, a weak
army, noon and midnight, a long walk, magic, trade, hiring, quests; a new goal every 20
actions). The model gets a text summary of Razdor's replay state (the hero; the nearest
buildings and armies with their cells and whether a click there is accepted now, found by
replaying the list with that click; open cells in 8 directions; the window on screen, found
from Razdor's notes on a probe `ok` / `wait`; the text of the event that just fired; the
last actions and the rejected ones) and answers `{"actions": [...]}` in the action list v1
plus `battle_act`/`battle_pass` (`battle_auto` is refused: a no-op in the original). Its JSON
is repaired (fences, trailing commas, aliases like `move`/`accept`, strings for numbers);
an action that is not valid on the screen, or that Razdor's replay skips, is dropped and
counted. Messages that only need OK are closed without asking; after three empty rounds a
fallback action is played. The model never judges results.

Then `run.py` plays the list on both sides (Frida `random` trace) and `known.py` sorts the
differences of the step-local run: `known:N` (FINDINGS.md entry N, by field and context;
the rules are in its docstring), `downstream:N` (a field entry N already threw off, or
everything after a battle-formation difference or a screen desync), `noise` (an AI army one
cell off with the generator in step: §5's frame noise), `timing` (a result of the event the
original still shows, which it applies at OK), `harness`, or `new`. For the first
`new` one the original runs once more on the prefix up to it (trace `random,ai,events`): if
it gives other values for those fields it was noise; else the prefix is shrunk by dropping
chunks of actions while the field still differs as `new` (`--shrink-budget` original runs),
and `~/.cache/razdor-difftest/explore/<id>/` gets the repro, both states at the step, both
screenshots, the trace of the step and the one before, and `candidate.json`; a short entry
goes into `CANDIDATES.md` for a human to confirm. A signature (the action's op and the
fields without ids) keeps the same candidate from being reported twice
(`explore/signatures.json`).

Files in `~/.cache/razdor-difftest/explore/`: `log.jsonl` (one line per episode: map, hero,
goal, length, the model's calls, invalid actions by kind, the classes found, times),
`<episode>-actions.jsonl`, `<episode>-chat.jsonl` (the model's replies), `runs/` (the
run.py folders).

Since the second version:
- **Services.** The model also gets the building window's content from Razdor's `--look`
  (goods, sell list, barracks, heal prices, spells), the book and the pack, and may answer
  with the service ops; goals push toward each of them.
- **The original plays along** (`LiveOriginal`, off with `--no-live`): each accepted action
  is played at once in the original as well (recorded as `original.py` records a run, Frida
  `random` trace on), so the explorer sees the original's screen; when it differs from
  Razdor's the prompt says what the original shows, and a window only one side shows is
  closed with `ok` / `answer no` (a resync step; the other side notes it and skips it). The
  diff then reuses that recording (`run.py --reuse-original`) instead of playing the
  original a second time.
- **Fights.** A click on a hostile army whose hit points pass the hero's army's by 1.3 is
  refused (`too_strong`). A battle lost (the game ends) is rewound: the actions from the one
  that opened the battle are dropped (the script's "reload of the last point before the
  battle"), that army's cell is avoided, and the episode goes on (at most 3 rewinds; the
  live original is restarted and replays the shorter list).
- `log.jsonl` also counts per op what Razdor applied (`ops_razdor`) and what the original
  applied without a note (`ops_original`), the rewinds, resyncs and screen mismatches.
- **Ollama settings.** Every request asks for the same context, `num_ctx` 16384, with
  `keep_alive` -1 (the model stays loaded) and `think` false: a request with another
  context size makes Ollama reload the model (about 80 s) and can push part of it onto the
  CPU. The prompts stay far below it: a busy one (a church with its goods, barracks and
  spells, 30 lines of history) is about 6,500 characters with the system prompt, some
  2,500 tokens; `cap_prompt` cuts a prompt over 24,000 characters from the middle, and
  `log.jsonl` records each episode's largest (`prompt_chars_max`).
- `--goals WORDS` keeps only the goals containing one of the comma-separated words (e.g.
  `--goals "(buy),(hire),(heal)"` for the service goals).
- A candidate's signature (the dedupe in `signatures.json`) includes the map.

First hour with this version (2026-10-03, all seven startable maps, the three classes in
turn): 9 episodes, 368 actions (8.5 episodes and 350 actions an hour, about 7 minutes per
episode, two thirds of it the model); 13% of the model's actions invalid (5 on the wrong
screen, 53 refused by Razdor, 3 outside the vocabulary; another 20 refused as too strong),
against 24% before; 6 of 9 episodes reached their length (4 of 13 before); 8 rewinds after
lost battles, 21 resyncs. Candidates: `CANDIDATES.md`, third round.

Known limits: the model plans on Razdor's state; a resync keeps the two sides on the same
screen but the step it fixes is still a difference (classified as usual); a rewind restarts
the original and replays the list (a minute or two).
