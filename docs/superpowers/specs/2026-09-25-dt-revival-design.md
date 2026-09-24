# Razdor as a Discord Times engine: design

## Goal

Turn Razdor from a prototype "inspired by" *Discord Times* into an open engine that plays the
original game's scenarios with the original rules, **reading the data from the player's own
installed copy at runtime**. The community can keep the game alive on modern systems, fix it and
extend it. The engine has no hard dependency on the original: without an install it still runs a
small built-in demo scenario made of our own content.

Success criteria:
- With `RAZDOR_DT_DIR` pointing to a Discord Times install (Community Update), the player picks any
  shipped `Maps_Rus/*.DTm` scenario and plays it: world map, buildings, armies, battles, economy,
  quests. The reference map for checking is `РК3-Столица` (the gameplay video) and the first
  playable map is `РК1-Начало пути`.
- Rules match the original as documented in `docs/reference/mechanics.md`. Where the original is
  unknown, the choice is marked *(guess)* in code comments and in that file.
- `cargo test` passes on machines without the game. Tests against the real files run only when
  `RAZDOR_DT_DIR` is set.

## Legal and content boundary

- The repo contains **no** original content: no maps, no `.ini` data, no text from them, no art,
  no sounds, no decoded or converted assets. Documentation describes file formats and rules in
  our own words (`docs/reference/`).
- Everything original is read at runtime from `RAZDOR_DT_DIR`. Nothing derived from it is written
  back into the repo. The optional asset cache (if ever added) lives in the user's cache dir.
- The built-in demo (`data/`) stays our own content.

## Reference material (in the repo)

- `docs/reference/dtm-format.md` – the `.DTm` scenario format, byte level, with confidence tags.
- `docs/reference/mechanics.md` – rules and data-file semantics (units, battle, magic, items,
  economy, scripting), from the shipped docs and a reading of the executable.
- `docs/reference/graphics-formats.md` – `.ugs` / `.lit` / `.spi` image formats.
- `docs/reference/video-notes.md` – observed UI flows and behaviour from gameplay footage.

## Architecture

```
src/
  dt/                 readers for the original's files. Pure, no macroquad, no game rules.
    container.rs      AIpf header + bzip2
    text.rs           cp1251 decoding
    ini.rs            ini parser (duplicate sections kept in order)
    data.rs           UnitDef / ArtefactDef / SpellDef / GlobalOptions from Rus_*.ini, _Global.ini
    dtm.rs            Scenario: header, terrain, objects, buildings, armies, points, events, strings
    gfx.rs            UGS / LIT / SPI decoders -> RGBA images
    install.rs        locate files inside RAZDOR_DT_DIR; load everything into DtInstall
  rules/              game rules, data-driven. No macroquad.
    content.rs        Content: unit/item/spell definitions + options, from dt::DtInstall or the
                      built-in demo; the rules only ever see Content
    ...               existing modules reworked (battle, units, items, world, game, clock, map)
    events.rs         scenario event engine (conditions, results, journal)
    fog.rs            explored / visible cells
  ui/                 macroquad screens; assets.rs draws original art when available,
                      placeholder tokens otherwise
```

Data flow: `dt::install::load(dir) -> DtInstall` → `rules::content::Content::from_dt(&install)`
and `rules::world::World::from_scenario(&scenario, &content)`. The built-in demo builds the same
`Content` and `World` from `data/`.

Crates added: `bzip2` (pure-Rust backend), `encoding_rs`. No other runtime dependencies without
need.

## Rules to implement (summary; full detail in mechanics.md)

- **Units** from `Rus_Units.ini`: hits, melee/ranged attack, magic power and school, melee/ranged
  defence, three magic protections (%), initiative, actions (`Manevres`), bonus ability, nature,
  regen, vampirism, per-level deltas, upgrade tree, cost. XP to next level
  `StartExpirience·(LevelMultipler/100)^L`.
- **Formation**: rows × columns configurable. Default **2×6** (the Community "wide row", as in the
  player's install and the video); vanilla **3×4 with a reserve row** supported. Reserve can't act
  or be targeted. Army cap 12. Back row +`Row2Def` defence vs shots.
- **Reach**: warrior in the front row hits enemy front cells c−1..c+1 at normal damage; if all
  three are empty, a long strike to the nearest front unit on either side that halves the target's
  defence (FlankStrike bonus doubles attack). Shooters/hostile mages from the back row target any
  enemy in rows 1–2; from the front row only adjacent enemies unless none are adjacent.
- **Damage**: deterministic `attack − defence`, min 1, with the bonus modifiers; knight hero −10%
  physical damage to his army. Magic by school with protection % and nature multipliers; mage power
  drains per turn to a floor.
- **Battle flow**: initiative order, attacker +1 initiative, `Manevres` actions per turn, move to
  c−1..c+1 in own rows, 25-turn limit, no retreat. The hero survives badly wounded while any unit
  lives; defeat = whole army dead. XP after battle per the pool formula.
- **World**: the scenario grid as 8-neighbour 32×22 cells (the data rule out a hex layout: diagonal
  bridge chains, unstaggered editor grid, 8-direction arrows; see `dtm-format.md` §4; *guess*),
  terrain costs *(guess, not in the data)*, objects: mountains and thickets block, hills and trees
  slow. Clock in minutes, 30-day months, 12 months, starting at the scenario's date. Villages
  refill at 00:00; the daily report (income, wages) at **12:00** as seen in the footage. Time passes
  while moving, waiting (1 h / 4 h) and casting world spells.
- **Buildings**: all 16 types; towns/castles/forts/villages/churches/markets/taverns/ruins with
  their screens (main hall with quests and rumours, barracks with stock and restock, market with
  fixed + random stock, 25% sell price, sanctuary spells, healing and resurrection costs), capture
  of castles/forts (owner, income), garrisons, ruins treasure.
- **Armies**: placed from the scenario, active/inactive, patrol radius, factions and relations,
  hostile armies chase and attack; hover shows the army's formation.
- **Economy**: gold and mana; building income; wages from unit cost; unpaid units sit out battles;
  victory loot = loser's gold ÷ 2 (min 25) and items.
- **Fog of war**: cells within the hero's sight radius become explored and stay visible;
  unexplored cells are impassable. Lanterns reveal areas. Minimap of explored cells.
- **Events/quests**: the event engine as in the editor manual (types, time window, conditions,
  results, flags, chained events, journal, victory/defeat events).
- **Spells**: spell book, learn for gold, cast for mana and game time on a whole army.
- **Save/load**: manual saves and an autosave before battles and at the daily tick.

## Stages

Each stage is implemented test-first, keeps `cargo test` green, is checked against the real
files with `RAZDOR_DT_DIR`, and ends in its own commit.

1. **Readers** (`src/dt`): container, cp1251, ini, data defs, full `.DTm` parse into typed
   structs with all 15 shipped maps parsing byte-exactly; `install.rs`.
2. **Content + battle rules**: `rules::content`, data-driven units and items, formation config,
   reach, damage, magic, bonuses (the vanilla 21 first), turn flow, XP and levels. Built-in demo
   re-expressed in the new model. Battle UI updated (card stats, reserve row, previews).
   2b (parallel): **graphics decoders** in `dt::gfx` + `ui::assets` loading original art.
3. **World from scenarios**: map select screen, terrain and objects, buildings, armies, hero
   preset start, new clock and daily ticks, terrain costs. *Done:* `World::from_scenario`
   (`rules/world.rs`), `map::Grid::Square8`, clock with noon report and midnight refill, waits,
   army contact battles, garrison battles and capture, scenario and class select screens,
   original terrain/objects/buildings/figures via one sprite atlas, path preview with travel
   time, bottom bar, army and building tooltips. Walkability checked on all 15 maps: every
   building of РК1 and РК3 is reachable; maps with islands (ДС1, ДС2, РК7, some of the
   tutorials) need ships.
4. **Buildings and economy**: building screens, capture, garrisons, income, wages, healing,
   resurrection, loot, markets, barracks restock. *Done:* `rules::town` (tabs per building
   type, hire with stock and regrowth, paid heal and resurrection, corpses, garrisons,
   sanctuary, village alternatives, dismiss), wages of both kinds and in mana, desertion,
   noon report and victory windows, loot and surrender mana, relation prices fitted to the
   footage, the building window and hero/army screen in `ui/`.
5. **Fog of war, minimap, army hover preview, waiting.**
6. **Event and quest engine**, dialogs, journal, victory/defeat. *Done:* the engine
   (`rules::events`) runs in the game (`rules::script`: `Game` as its world; ticks at the
   start, after every slice of time, on entering a building or point, after battles,
   answers and rumours), story and question dialogs with rewards and pictures, quest notices,
   the journal screen, rumours for 10 gold in main halls, victory/defeat end screens. Lantern
   reveals are recorded in `Game::pending_reveals` for the fog of war.
7. **Spells on the world map; save/load.**

## Out of scope for now

Ships and pirates, AI feudal-lord economy beyond simple chase/patrol, AI army respawn, the
Community extra event opcodes, sound and music, map editor.
