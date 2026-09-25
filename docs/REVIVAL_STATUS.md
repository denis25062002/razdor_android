# Razdor: Discord Times revival, status report (2026-09-25)

Razdor is now an open engine for *Discord Times* («Времена раздора», Aterdux, 2004). It plays
the original scenarios with the original rules, reading maps, unit/item/spell data and art from
**your own installed copy** at runtime. Without an install it runs a small built-in demo made of
our own content.

Branch: `dt-revival` (20 commits on top of `main`; `main` is untouched). About 27 k lines of Rust.

## How to play

```sh
export RAZDOR_DT_DIR="/path/to/Discord Times Community Update"   # folder with Maps_Rus/, Rus_*.ini, Graphics/
cargo run --release
```

Pick a scenario (the built-in demo or any map in `Maps_Rus`), then a hero class from that
scenario's presets; type a name for the hero on that screen if you like (`#HERONAME` in the
texts; empty means the class's name). Unset `RAZDOR_DT_DIR` to play the demo only.

Controls: click the map to walk (a dotted route shows the travel time); right click / Space
stops; wheel or +/− zooms; 1 / 4 wait 1 or 4 hours; M minimap; J journal; B spell book;
N music off/on; Esc menu (save, load, main menu, music and sound volume). In battle: click a framed card to attack or cast (right click picks
the other action), a lit cell to step there, Space to end the unit's turn.

## What is in

| Area | State |
|---|---|
| **Original files** | `.DTm` scenarios (all 15 shipped maps parse byte-exactly and re-serialise identically), `Rus_Units/Artefacts/Spells.ini`, `_Global.ini`, art (`.ugs`, `.lit`, `.spi`; all 421 files decode, pixel-identical to the reference decoder). |
| **World map** | Scenario terrain, trees, hills, mountains, buildings and army figures drawn with the original art. Movement as the original's code (docs/reference/original-mechanics/world.md): 8-way squares, diagonal ×1.5, the original's terrain and object costs (road 15, grass 25, marsh 40 minutes; shallows are water), massifs cover squares; building footprints are walked at road speed and entered from any cell; only ill-disposed castles/forts and ruins not yours bar the route. Routes with travel time; tooltips; zoom. |
| **Time** | The original calendar (30-day months, days from 0, hours). Time passes while walking (each step is charged the cell left × the hero's speed: knight/archmage 5, ranger 4) and waiting (30-minute ticks), each step or tick played over 150 ms; healing and resurrection take no time. At 00:00 villages refill (slower as they fill), barracks may gain a unit, garrisons heal; the income/wages report and an autosave come at 12:00. |
| **Ships** | Shipyards rent a ship for `ShipCost` (250 gold). Walk onto it to board, click the water to sail (shallows and coastal water; deep sea blocks ships), the shore to land; the ship waits on the water where he stepped ashore and takes him back when he walks onto it. Pirate, merchant and hero ships of the scenarios sail and cruise; pirates attack, merchants never do. Saved with the game. |
| **Fog of war** | Unexplored land is black with a soft edge and cannot be walked; the hero feels his way into the dark; lanterns and scripted reveals light areas. Minimap of the explored land with owner-coloured icons. |
| **Armies** | Placed from the scenario, active/inactive, factions and attitudes; hostile armies chase and attack; friendly ones greet (events run on meeting). |
| **Buildings** | All 16 types. Building window with the original's tabs by type: main hall (quests, rumours heard for free; a rumour's own event may cost gold), barracks (stock that regrows by the original's daily roll, paid healing, resurrection within 7 days), garrison, market + sell shop (the original's attitude price table, stock drawn anew every midnight, towns stock healing potions), sanctuary (learn spells), village tribute (gold and mana, growing by the original's √ rule) and the one offer a visit may bring (innkeeper, priest, long blessing, furs or witch, by the original's rolls), shipyard (rent a ship). Forts at the foot of bridges let you through to the bridge. Beating a garrison makes the place the hero's: forts and castles change owner and income, ruins give their treasure and become his. |
| **Economy** | As the original's code computes it (docs/reference/original-mechanics/economy.md, `src/rules/economy.rs`): the player's units are recruits, the hero and event units free, garrisons unpaid; a short noon refunds the cheapest units and sets the gold to 0, and units unpaid for 7 days leave; the difficulty factor on healing, resurrection, sales and income; Rear Service; villages linked to your castles pay at noon; loot = the army's gold ÷ 2 plus its daily wages and items, and its empty home castle; mana from surrendered units. |
| **Battle** | The original's rules as reverse-engineered in original-mechanics/battle.md (every row of its table): the Community wide row (front 6, back 4, reserve 2, drawn 2×6 with the reserve at the back row's ends; vanilla 3×4 + reserve supported), the descending initiative scan (ties and +1 to the player, Artillery +30 on turn 1), modifiers until the next turn, automatic mage actions (curse, then strike; heal, else bless), one reserve move per unit and turn, collapse after deaths and last actions, the piercing sets per path, Knight −20%, poison as −20 regeneration, surrender (mana from the surrendering units only), the turn limit as a victory, one bonus per unit, all vanilla and Community bonuses with the exe's numbers, and the deterministic scored AI that never uses the reserve. Cards show the original's stats; hover previews the action. |
| **Units** | Experience as the original's code computes it (docs/reference/original-mechanics/experience.md): unit strength from the stats, the battle pool and shares by row and activity, the player's modifier × difficulty × the beaten army's correction with the Community 5256 cap, victory only; AI-vs-AI XP, AI promotion and XP for AI hires (map bytes 14, 19); levels (`StartExpirience·(LevelMultipler/100)^(level−1)`, `d-*` gains, percent stats), promotions along the upgrade tree (any non-hero unit with a level, free, back to level 1), level-up notices; XP bars and "Lv N · XP a/b" on cards, panels and lists, "Level up!" after a battle, the upgrade tree on the army screen, 4 item slots with the one-weapon / one-per-type / class rules, `f-`/`d-`/`p-` modifiers (percentages compound per item), potions, 256-slot backpack, hero class bonuses (knight −10% physical damage to his army, archmage cheaper faster spells, ranger faster and better healing). |
| **Events and quests** | The scenario script engine: global / local / quest / rumour events, time windows (in hours) and repeats as the original checks them (a No counts as happened, events without "once" fire on every check), relative and chained events, all condition and result groups, counter flags (`%+X -X =X =/X`), yes/no questions, journal, victory and defeat events. The Community extensions: event opcodes 1–20 (editing other events, AI armies' items/units/speed/groups/spells/XP, spell checks, campaign branches with `Game::next_map()`, random flags, AI targets, teleports) and lifting a spell. Story dialogs with pictures and rewards. On РК1 the opening dialog, the first quest and the journal work. |
| **Spells** | Spell book on the world map; cast on your army or a nearby hostile army for game time, the mana taken when the spell completes; effects last into battles, a recast adds time, 4 per unit; `OneEnemy` and life drain hit only the leader; instant damage can kill; archmage or Caster discount (not both); event and village spells last 10× (5×) as long. |
| **Sounds and music** | `_Sounds.ini` and `Sounds/` read at runtime (`.wav` as is, headerless `.raw` wrapped in a WAV header in memory, 22050 Hz, `RAZDOR_MUSIC_RATE` to override). Menu theme; the seven map themes shuffled; battle themes; triumph after a won battle and at victory; defeat. Effects for buttons, windows, the battle horn, every battle action (cannon by `ShotWeaponRange`), card moves, event chords, level-ups, spells good/evil, items by type, gold. N mutes the music; volumes and mutes in the Esc menu, kept in `audio.json`. |
| **AI armies** | The scenario's armies choose goals as the original does: every candidate seeded with its `_Global.ini` priority into one flood, lowest priority + path cost wins; armies and the player within `AIDistance0..2` cells by behaviour style (feudal / rogue / peasant, byte 59), inside the patrol box; an attack only when a simulated battle is won. Goals: attack, take castles and forts (rogues retake their home fort), heal, garrison, hire, shop, collect tribute, talk, patrol, go home; all five editor flags. They walk on minutes banked from your steps (speed `max(1, 5 − correction)`). Feudal economy: income (byte 80 × 10), wages, a 5-day reserve, hiring and buying items. AI-vs-AI battles, captures, reports within sight. Beaten lords retreat and return; armies respawn at their home building's centre. 30 simulated days take 0.01–0.8 s per map (release). |
| **Map editor (steps 1–2)** | `--editor` or "Map editor" on the title: new/open/save `.DTm` maps that load in the original and in Razdor (all 15 shipped maps re-save byte-identically); terrain brushes, fill and rectangles, objects, buildings with their pictures' footprints, armies, points, hero starts; property panels for every building, army, point and scenario setting of the original editor's forms; **the event editor**: the event list (type / group / title filters, new, duplicate, delete with every reference renumbered and a warning when the event is still used) and every field of the original's event window on its four tabs (each control mapped to its byte by reading the original editor's save routine), attaching events to buildings and points, the Community opcodes 1–20 with named arguments, imported event pictures; undo/redo; checks before saving (every id an event names, quests, flag scripts, string counts); test play in Razdor. Saves go to `~/.local/share/razdor/maps` (`RAZDOR_MAPS_DIR`); the game folder only by an explicit, confirmed action. No random map generator (not planned). Design: `docs/superpowers/specs/2026-09-25-map-editor-design.md`. |
| **Saves** | Manual saves and autosaves (before every battle, at every noon; newest 10 kept) in `~/.local/share/razdor/saves` (or `RAZDOR_SAVE_DIR`). A save refers to the map by name + hash and re-reads it from your install. |

Tests: **506 library + 19 app tests** pass with and without `RAZDOR_DT_DIR`; tests on the real
files run only when it is set. `cargo clippy --all-targets` is clean.

## Decisions I made on my own

- **The wide row by default** (front 6, back 4, reserve 2), because your Community Update install
  and the video use it; the vanilla 3×4 with a reserve row is a supported option.
- **Square cells with 8 neighbours for scenarios** (confirmed by the exe, world.md §1). The
  demo keeps its hex map.
- **Ships** (*(guess)*, mechanics.md §8.6): a rented ship waits at the water nearest the
  shipyard on foot; ship armies always cruise; merchants never attack; a preset on the water
  starts the hero aboard. The earlier guesses about gates, entry cells and start buildings are
  replaced by the original's rules (world.md §1, §7): a fort at the foot of a bridge is walked
  into (fight its garrison, or just enter a neutral one) and out on the far side.
- **AI** (mechanics.md §8.8): army byte 59 is the behaviour style; the scoring, ranges,
  walking and respawn now follow world.md §4–5. Still *(guess)*: how aggression shifts the
  simulated battle, no repulsion field around a losing target, lords recover 3 days in a
  building, an army's items are worn by its units in battle, a captured castle gets a
  garrison of the taker's weakest troops.
- **Reachability with the original's rules** (env-gated test, by land and rented ships):
  unchanged on 11 maps; newly out of reach: the first tutorial's three villages (fords are
  water), РК3/РК5's altar (massif squares and a lake without a shipyard) and РК7's eastern
  island (deep sea). Scripted teleports are not counted; check these in the original.
- **Daily report at 12:00**, villages at 00:00: the footage shows the report and autosave at noon.
- **A beaten unit gives its `Surrender` value in mana** (fitted to the footage; the battle
  code's rule is not traced). Prices, wages and loot now follow the executable
  (original-mechanics/economy.md).
- Everything the original leaves unknown is marked *(guess)* in the code and listed in
  `docs/reference/mechanics.md` §8 (level numbering, …).

Closing the window used to end in a segmentation fault (a native library's exit handler,
after `main` returned); the game now ends the process directly once everything is written.

## Not done yet

- **Ships** are drawn as a placeholder shape (hull and sail), not the original's ship
  sprites (`Ship-*.ugs` are read but their sheet layout is not decoded). What the "hero
  ship" type (army byte 72 = 1) means is unknown; such ships sail like friendly armies.
- The second tutorial's church (building 15) stands in a ring of dense thickets and bog:
  unreachable unless some thicket sprites are passable in the original.
- **AI**: "beaten by anyone" in the event engine still reads only the player's battles: the
  one-line switch to `Game::army_beaten_by_anyone` belongs in `src/rules/script.rs`, which
  another branch (Community opcodes) is changing; apply it after that merge (a ready patch:
  `/tmp/claude-1000/-home-indicozy-Documents-projects-razdor/9ee4204d-b333-4316-953b-dbb147f5fd94/scratchpad/army-beaten-by-anyone.patch`,
  `git apply` it from the repo root). Everything
  about how the original weighs its AI priorities, uses `AIDistance0..2` and garrisons is a
  guess (mechanics.md §8.8). AI ships only cruise and chase the hero.
- **Economy gaps** (economy.md, "Razdor now"): AI hires in foreign buildings are not
  mercenaries (kind 2) yet, castle and fort stocks are not modelled (they pay their daily
  income), the market's lean towards dear goods is not reproduced, and the Community
  "set a flag digit" patch is not in. The tribute tab's offer button was built without
  opening a window.
- **Community bonuses and opcodes** are in, but no shipped unit, item or map uses them, so
  they are tested on made-up data only. `Dominate` is undocumented (a guess), several sizes
  are guesses (mechanics.md §8). Not wired outside the rules: the "Next map" button on the
  victory screen, `NoHeal` blocking the world's healing, opcode 8 on the hero's own speed,
  `set_in_building` for garrisons without extra defence (one line in `game.rs`). AI units
  carry no own items or spells, so opcodes 6 and 11 act on whole armies.
- Soft terrain transitions.
- **Map editor**: the random map generator is not planned. Not built: "test play from here" for an event, exporting event pictures. **The editor window was not seen**: layout, panels, drop-downs, the canvas and the new event window (list, six tabs, pickers) were built with tests, clippy and a release build only; check them on a real screen first.
- **Event duration units** (for review): the original editor stores an event's "open for N hours" as N × 60, but the game's window check reads the stored number as hours, so an event the editor shows as open 24 hours stays open 1440 hours in the game. Razdor's engine follows the game; the event window shows both numbers.
- Sounds: the menu bells (`MainMenuSelect-*`), the scroll sound and `BkgAuthors` (no
  credits screen) are not used yet. **Nobody has listened yet**: the sound was checked by
  logs, a decode round trip and quad-snd loading every file at volume 0.
- **Experience UI not seen**: the XP bars, level labels, "Level up!" badges, the level-up
  notice on the map and the upgrade tree view were built without opening a window (tests,
  clippy and a release build only). Check their layout on a small window first.
- **Not verified by a human**: your screen was locked, so all visual checks used offscreen
  snapshots of real game frames; nobody has clicked through a full scenario yet. Please play
  РК1 first and note anything off.

## Content boundary

The repo contains no original content: no maps, `.ini` data, text or art. The built-in demo
(`data/units.ini`, `data/items.ini`, `data/spells.ini`, `data/kingdom.txt`) is our own (names
checked against the original's: no overlap). The reference docs in `docs/reference/` describe
formats and rules in our own words. Saves hold only a reference to the map.

## Documentation

- `docs/superpowers/specs/2026-09-25-dt-revival-design.md` – the design and stages.
- `docs/reference/dtm-format.md` – the scenario format, byte level.
- `docs/reference/mechanics.md` – rules and data semantics; §8 lists every Razdor guess.
- `docs/reference/original-mechanics/` – the original's rules read from the executable
  (battle, world, economy, experience), each with a "Razdor now → original" table.
- `docs/reference/graphics-formats.md` – the art formats.
- `docs/reference/video-notes.md` – observed behaviour from the gameplay video.

## Cleanup for you to run later

Nothing was deleted while you were away. These are safe to remove:

```sh
cd ~/Documents/projects/razdor
# merged worktrees and their branches
git worktree remove .claude/worktrees/agent-a80832d6685e0f106
git worktree remove .claude/worktrees/agent-adaff433a9427fd72
git branch -d worktree-agent-a80832d6685e0f106 worktree-agent-adaff433a9427fd72
# session scratch: research notes, decoded maps, converted art, screenshots (outside the repo)
rm -rf /tmp/claude-1000/-home-indicozy-Documents-projects-razdor/
# a HEAD checkout used to compare test timings, and its build directory
git worktree remove /tmp/claude-1000/-home-indicozy-Documents-projects-razdor/9ee4204d-b333-4316-953b-dbb147f5fd94/scratchpad/base
rm -rf target/basecmp
# a core dump from an offscreen snapshot run, if systemd kept one
coredumpctl list razdor
# temp folders left by the editor's tests (never the repo or the game folder)
rm -rf /tmp/razdor-editor-*
# the DFM parser venv used to read the original editor's forms, and the editor's
# disassembly and event decoders used for step 2
rm -rf /tmp/claude-1000/-home-indicozy-Documents-projects-razdor/9ee4204d-b333-4316-953b-dbb147f5fd94/scratchpad/editor
rm -rf /tmp/claude-1000/-home-indicozy-Documents-projects-razdor/9ee4204d-b333-4316-953b-dbb147f5fd94/scratchpad/ev2
```

Keep the scratch folder if you want the research notes (`RULES.md`, `DTM_FORMAT.md`, the Python
decoders); the cleaned versions of the notes are already in `docs/reference/`.

## Next steps I'd suggest

1. Play РК1 end to end and report problems.
2. Watch the AI on a real playthrough: whether lords are too busy or too idle.
3. Wire the campaign "Next map" into the victory screen.
4. Merge `dt-revival` into `main` once you're happy, and publish for the community.
