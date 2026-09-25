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
| Point settings (`TwPoints`, `TInfoTarget`) | attached events (≤5), start radius, active at start, 4 target priorities, active duration | point bytes 8–40 | **step 1** (priorities/duration: always 0 in shipped maps; kept, not shown) |
| Named characters | list of (name, class), add/edit/delete | header 0xEE/0xEF, strings | **step 1** |
| Events (4 tabs: event and player, event and heroes, result 1, result 2) | type, time window, repeat, archetype, all conditions and results, flags, question, message, picture | event bytes 0–170, 3 strings, pictures | **step 2** |
| Map generator (`TMakeMap`), world generator (`TMakeWorld`) | size, relief type, orientation, ratios, seed, smoothing; building counts, economy, armies | whole map, header 0x14 | **step 2** |
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

## Architecture

```
src/editor/            pure model, no macroquad (lib crate, fully tested)
  doc.rs               EditorDoc: Scenario + origin + undo/redo + save; Command execution
  command.rs           Command enum, Settings, touched Sections
  tools.rs             Tool, ToolState: press / drag / release on cells -> Commands
  geometry.rs          brushes, rectangles, flood fill, Footprint, object cover
  refs.rs              remapping ids when a building/army/point/named character is removed
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
byte of such events is left alone).

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
input. Env-gated: all 15 shipped maps open → save → identical file; edit + undo → identical;
edits of РК1 still build a game; every building's footprint equals its sprite's.

## Step 2 (described, not built)

- **Event editor**: list with type/colour filters, new/duplicate/delete (with id remapping of
  every event reference and of buildings'/points' local lists), the four tabs of conditions and
  results, flag scripts in the title, question and message texts, pictures (standard list or an
  imported 128×128 RGB565 image), Community opcode helpers.
- **Random map generator**: size, relief type and orientation, the terrain ratio sliders, seed
  and smoothing; then infrastructure (building counts, incomes, markets, libraries, garrison
  strengths) as the original's "world" generator.
- Fog-of-war preview, playability check (reachability of buildings from each start using the
  rules' pathing), resizing maps, importing/exporting the scenario picture.
