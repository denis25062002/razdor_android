# Razdor map editor: design

## Goal

A community tool that creates and edits `.DTm` scenarios which load in **both** the original
*Discord Times* and Razdor. It is part of Razdor (`razdor --editor`, or "Map editor" on the title
screen) and reads the player's install at runtime for art, names and the game's maps folder.

Legal boundary (as in `2026-09-25-dt-revival-design.md`): nothing from the install goes into
the repo. The editor's UI, layout and English labels are our own. Names of units, artefacts,
spells, buildings and events that the UI shows come from the user's files at runtime.

## What the original editor does (from its forms)

The original `DTMapEdit.exe` is a Delphi program; its window layouts are Delphi form resources
(`TPF0` streams in `.rsrc`, 22 forms). They were parsed to list every panel and field, and
the manual and `Rus_MapEdit.ini` were read for what each does. Summary, with the `.DTm` field
each edits (offsets in `docs/reference/dtm-format.md`):

| Original window | Fields | `.DTm` | Razdor |
|---|---|---|---|
| Main window | 5 palettes (surfaces; hills/mountains/rocks; forests; buildings; armies and events), brush 1–6, Info/Del/Move modes, grid, patrol zones, fog view, minimap, new/open/save/generate | terrain RLE, objects, records | **step 1** (brushes 1/3/5/9 + fill + rectangle; fog view later) |
| Scenario settings | title, description, victory and defeat events, map size, start date; per class: gold, mana, "experience", start building, troops 6×(unit, level, count), 3 artefacts, 5–6 spells, coordinates; relations 4×4 with presets; campaign: kind, campaign name, next map file, 7 carry-over boxes, scenario picture | header 0x38, 0x3C presets, 0xD2, 0xD8, 0xDE, 0x10F–0x120; strings 1–4 | **step 1** (picture: index only; embedded LIT picture kept, not edited) |
| Building settings | name, neutral owner, description, picture, owner army, start-owned per class, type; tabs: local events; barracks 6×(unit, count, available) + "all types"; garrison 6×(unit, level, count), extra defence, AI-only; treasure (5 artefacts, gold); market 6 goods, random count, price range; spell library 6; factions and attitudes; gold/mana income and max; tunnel (linked building) | building bytes 0–357, 3 strings | **step 1**, all fields |
| Army settings | name, leader name, base class, named character, artefacts 3, start building, spell, ship picture, start level, gold; troops 6×(unit, level, count), description, cost/strength display, gold income; AI: behaviour, factions and attitudes, hire XP + "like the player", garrison strength, inactive, patrol + radius, speed correction, aggression, target model, 5 target flags, respawn time + all, XP correction, no money | army bytes 0–88, 3 strings | **step 1**, all fields (tactical costs 6/74 displayed, kept) |
| Point settings (`TwPoints`, `TInfoTarget`) | attached events (≤5), start radius, active at start, 4 target priorities, active duration | point bytes 8–40 | **step 1** (priorities/duration: always 0 in shipped maps; kept, not shown); attaching by title in **step 2** |
| Named characters | list of (name, class), add/edit/delete | header 0xEE/0xEF, strings | **step 1** |
| Events (`TInfoEvents`: list with type and colour filters, new/copy/save/delete, title; 4 tabs: event and player, event and heroes, result 1, result 2) | every input control, each mapped to its byte by the editor's save routine (dtm-format.md §9) | event bytes 0–150, 163; 3 strings; pictures | **step 2** (all fields; see below) |
| Map generator (`TMakeMap`), world generator (`TMakeWorld`) | size, relief type, orientation, ratios, seed, smoothing; building counts, economy, armies | whole map, header 0x14 | **not planned** (header 0x14 is kept as it is) |
| Artefact / unit settings | `Rus_*.ini` editors | not in `.DTm` | later (not map data) |
| AI test, battle test, error list, options | debugging views | – | error list = our **Check** window; the rest later |

## Step 1 (built)

- **Documents**: new map (50/100/200 or custom up to 800, one surface), open (the install's maps,
  the user's maps, or a path), save, save as, save to the game folder, unsaved-changes prompt.
- **Tools**: select/move (drag keeps the grab offset), terrain (16 surfaces; brush 1/3/5/9, flood
  fill 4-connected, rectangle), objects (class + sprite from `Objects.ugs` keys, brush sizes,
  per-cell stacks, no duplicate on a cell), erase (all / massifs / plants), buildings (type +
  picture with the sprite's footprint, preview green/red), armies, points (lantern / event point),
  hero start picking.
- **Panels**: building, army, point, scenario settings with every field of the forms above
  except events; pickers of units, artefacts and spells named from the content; local event
  lists edit ids only.
- **Canvas**: the original art through `DtArt` (terrain textures, object and building sprites,
  army figures) or placeholders; far zoom and the minimap use a one-texel-per-cell terrain
  texture; grid (G), massif cover squares (H), patrol radii (R), lantern radii, hero starts,
  selection and footprints (with the extra top row of wide buildings).
- **Check**: validation list; click an entry to go there.
- **Test play**: the in-memory scenario starts a Razdor game (`Game::from_scenario`) with the
  install's content (else the demo's, which rarely fits a real map); "Main menu" returns to the
  editor with the document intact.

## Step 2 (built): the event editor

The **Events** button opens a window with the event list on the left and the selected event on
the right. Everything the original's `TInfoEvents` form edits is there; the grouping follows its
four tabs, and two tabs of our own are added.

| Original control(s) | Byte | Razdor |
|---|---|---|
| list, `RadioButton_All/Global/Local/Quest/Gossip`, `ComboBoxColorGrupp` | – | list with type toggles, group picker and a title search (flag scripts are not searched); ids in order |
| `sbNew`, `sbCopy`, `sbDel` | – | New (of the filter's type), Duplicate (picture included), Delete (see below) |
| `EditNameEvents` | string 1 | the name part of the title; the flag parts stay |
| `RadioButtonGlobal/Local/Quest/Gossip` | 1 | Type |
| `ComboBoxGrupp` | 0 | Group (6 colours) |
| `MaskEditDate`, `CheckBoxRelativTime` | 2 | start year/month/day (0-based)/hour, or "relative only" = start 1 036 800 000 |
| `RxSpinEditDayRepeatTime` (hours) | 8 | open for N hours, stored × 60; the game reads the stored number as hours, shown as a note |
| `RxSpinEditDayRepeat` (days, ≤31) | 6 | repeat every N days, stored × 1440 |
| `CheckBoxRepeat` | 141 | "can happen many times" = once byte 0 |
| `CheckBoxSlave` | 140 | subordinate |
| `RadioGroupHeroType` | 10 | for which hero |
| `CheckBoxEventsYes/No`, `CheckBoxNotEvents` + 2 combos each | 56–74 | check + two event pickers |
| `EditGetFlag` / `EditSetFlag` | title | "check flag" (tab 1) and "flag" (result 1) fields; the title is rebuilt as the original writes it (a flag without + or − is dropped, with a warning) |
| `CheckBoxAnswer`, `MemoQuest`, `CheckBoxRepeatQestion` | 76, string 2, 149 | question box, text, ask again after yes |
| `CheckBoxVictory` + 2 combos, `ComboBoxArmyMeet` | 53–55, 74 | beaten armies, meeting |
| `CheckBoxHeroRes`, `RxSpinEditExperience/Gold/Blessing` + switches, `RxSpinEditUnit/Cost` + switches | 18–26, 11–14 | stats check; level, gold, mana, squads (≤12), strength with a ≥/≤ switch (the sign) |
| `CheckBoxHeroIll` | 145 | "the hero is left with 1 HP" (on tab 1, as in the original) |
| `CheckBoxBilding`/`Artefact`/`Army` + 3 × (id, [name,] owner) | 29–52 | ownership conditions; owners (any), player, green, blue, yellow, red, not the player |
| `CheckBoxDead` + 2, `ComboBoxArmyLive/NoActive/OnHome` | 66–68, 75, 15, 146 | beaten by anyone, active, inactive, at home |
| `MemoInfo`, `ComboBoxNextEvents`, `ComboBoxEndQuest` | string 3, 138, 124 | message, chained event, quest completed (quests only) |
| `RxSpinEditSetExperience/Gold/Blessing` | 83, 85, 89 | XP, gold, mana (opcode arguments when an opcode is set) |
| `ComboBoxSetEventsTime`, `RxSpinEditSetEventsTime` (≤5000), `RxSpinEditTime` | 77, 79, 126 | relative event and its delay in hours; the hero's wait |
| `ComboBoxAddUnit1–4` + names, `ComboBoxUnitFromArmy`; `sbPersonalizedUnit` | 97–104, 142 | units joining with named characters, the army they come from; named characters are made in Settings |
| `ComboBoxNewSpell1–4`, `ComboBoxNewArtefact1–4` | 93–96, 113–116 | spells learned, artefacts gained |
| `ComboBoxDelUnit1–4` + names, `ComboBoxUnitToArmy`, `ArmyGoToHero` | 105–112, 136, 143 | units leaving ("a unit an event added" 0xFE, "any unit" 0xFF), where they go, move that army to the hero |
| `ComboBoxDelArtefact1–4`, `ComboBoxFogLamp1–4` | 117–120, 128–135 | artefacts lost, lanterns lit (points) |
| `ComboBoxShowArmy`, `ComboBoxArmyActive1–2`, `ComboBoxArmyDeActive` | 144, 121–123 | show, activate, deactivate |
| `ComboBoxPatrulArmy`, `RxSpinEditPatrul` (±120) | 16, 17 | patrol change (replaced by a note when it selects an opcode) |
| `ComboBoxNewHeroType`, `ComboBoxCastSpell` | 137, 81 | new hero class, spell on the player's army |
| `ComboBoxBattleArmy`, `CheckBoxBattleArmyGenerate` | 147, 150 | battle with army; "generate that army to match the player" (byte 150, found in the save routine) |
| `CheckBoxBreak` | 148 | no meeting (a "!" note when the event also needs a meeting, as the original warns) |
| `ComboBoxStandartImage`, `ImageEvents`, `bbNew`/`bbDel`, `OpenImageDialog` | 82, 163, picture | standard picture (none, victory, defeat, unit portraits); import a PNG (scaled to 128 × 128 RGB565) or remove it |
| – (our own) *Places* tab | building 8/288, point 8/39 | attach to / detach from buildings and points (≤5 on a point), and the list of what refers to the event |
| – (our own) *Community* tab | 17, 148, 83–90; 11–26 | the Community opcodes 1–20 by name, their three arguments labelled (holders, armies, events by relative shift with the target's title, event fields by name, named characters, units), the second "edit event" setting of opcodes 1–5 and the position check of 19 |

Buildings' and points' panels attach events picked by title (with their type). Scenario
settings pick the victory and defeat events. Not built: "Test play from here" (test play starts
at the scenario's start), exporting an event picture, the original's "Move" button of the list
(`sbMove`; its use is unknown).

**Deleting an event** (`refs::remove_event`): later ids move up one and every reference is
remapped: other events' happened yes / no / not happened, relative, quest completed and chained
events, buildings' and points' local lists (the entry is taken out and the list closes up),
the victory and defeat events, and the relative shifts of the Community opcodes 1–5 (both
settings; a shift that pointed at the removed event is kept). References to the removed event
become none. If anything still refers to it, the editor lists where and asks first.

**Validation** added: every event id, army, point, building, named character, unit, artefact and
spell an event names exists (the units removed may be 0xFE/0xFF, the picture 200/201); "quest
completed" must name a quest; type 1–4, archetype 0–3, owner codes 0–6; the flag script's
syntax (`+X`/`-X`, `=X`/`=/X`, no empty or spaced names, one `%`); at most 5000 events; the
payload's string count equals 4 + 3 per building, army and event + 1 per named character.
Warnings: opcode arguments that name nothing on the map (armies, buildings, the target event,
the field offset), a question box with neither question nor message text, a picture whose size
does not match its data. All 15 shipped maps still validate without errors.

## Architecture

```
src/editor/            pure model, no macroquad (lib crate, fully tested)
  doc.rs               EditorDoc: Scenario + origin + undo/redo + save; Command execution
  command.rs           Command enum, Settings, touched Sections
  tools.rs             Tool, ToolState: press / drag / release on cells -> Commands
  geometry.rs          brushes, rectangles, flood fill, Footprint, object cover
  refs.rs              remapping ids when a building/army/point/named character/event is removed
  events.rs            event editing: new events, time units, thresholds, title and flag
                       script, Community opcodes, references, the list filter, pictures
  records.rs           slot lists (local events), goods, army strength
  defaults.rs          new map and new records
  palette.rs           Palette from Objects.ugs (or fallback), Names from Content, labels
  validate.rs          Issue list (errors block saving) + writer self-check
  files.rs             save locations, confirmations, atomic writes
src/ui/editor/         macroquad window
  mod.rs               EditorScreen: layout, toolbar, dialogs, shortcuts, save flow, test play
  canvas.rs            Cam, map drawing, overlays, minimap
  palette_panel.rs     tool palette
  props.rs, form.rs    property panels and their form layout
  settings.rs          scenario settings window
  events.rs            the event window (list, tabs, pickers)
src/ui/widgets.rs      + text, number, check box, drop-down (popup), tabs, focus
```

`App` has `Screen::Editor` and keeps the `EditorScreen` during test play (`test_play` flag).

## File handling

- Written with `Scenario::to_payload` + `container::encode` (bzip2 level 9). Opening and saving
  any of the 15 shipped maps unchanged gives a **byte-identical file** (tested).
- Before writing: validation, then a self-check that the payload re-reads and re-serialises to
  the same bytes. Files are written to a temporary name and renamed.
- Default folder: `RAZDOR_MAPS_DIR`, else `~/.local/share/razdor/maps`. A map opened from the
  game's `Maps_Rus` is marked as a game map and "Save" goes to the user's folder.
- The game folder is written only by "Save to game folder": first a confirmation, then — if a
  file of that name exists there (case-insensitive) — a second one. Ctrl+S on a map saved to the
  game folder asks both again. Replacing a different file in the user's folder asks once.

## Undo model

Each command declares the scenario parts it may change (terrain, objects, buildings, armies,
points, events, meta = header + scenario strings). Those parts are copied before it runs; undo
swaps the copies with the current parts (which become the redo entry). Commands that change
nothing leave no entry. A group (a brush stroke, a drag) makes one entry; a merge key (record +
field) folds typing into one entry. 200 steps are kept; "dirty" compares the top entry with the
saved one, so undoing back to the saved state is clean.

## Deleting records

Ids are positions. Removing building/army/point/named character `k` shifts later ids down and
remaps every reference listed in the format doc (army home buildings, linked buildings, preset
start buildings, owners, event conditions and results, lanterns, named units); references to `k`
become none (owner: 0xFF). Arguments of Community opcode events are not remapped (the patrol
byte of such events is left alone), except the relative event shifts of opcodes 1–5 when an
event is removed (step 2 above).

## Validation

Errors: map size and terrain length, codes > 15, objects/records outside, footprints outside
(extra top row included), ids out of order, dangling references (buildings, armies, events,
points, named characters), unknown units/artefacts/spells (with install names), pictures the
game lacks (with the install palette), too many records (255) or named characters (32), NUL in
strings, gold/mana > 32767, relations outside −3..3, bad factions. Warnings: non-square map,
overlapping buildings, footprint ≠ picture's, barracks start > max, price range reversed,
characters Windows-1251 cannot store. All 15 shipped maps validate without errors.

## Testing

Unit tests for every command, undo/redo, groups and merging, dirty tracking, deletes with
remapping, validation rules, save locations and confirmations (temp dirs only), property edits
landing at the documented byte offsets, the tools state machine, camera math and text/number
input. Step 2 adds: new/duplicate/delete/set/attach/detach events with undo and redo, the
renumbering of every event reference site (opcode shifts included), the time units, thresholds,
title and flag script composition, the opcode fields' round trip through the engine's decoder,
every event field landing at its offset in `to_payload` and reading back, and the new
validation rules. Env-gated: all 15 shipped maps open → save → identical file; edit + undo →
identical; add + delete an event and duplicate + delete → identical; deleting each map's most
referenced event leaves no error, and the event engine runs the edited map for three game
days; edits of РК1 still build a game; every building's footprint equals its sprite's.

## Later (not built)

- **Random map generator**: not planned (the original's `TMakeMap` / `TMakeWorld`; maps made
  elsewhere open and save as any other).
- Fog-of-war preview, playability check (reachability of buildings from each start using the
  rules' pathing), resizing maps, importing/exporting the scenario picture, "test play from
  here" for events.
