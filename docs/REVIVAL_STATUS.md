# Razdor: Discord Times revival, status report (2026-09-25)

Razdor is now an open engine for *Discord Times* («Времена раздора», Aterdux, 2004). It plays
the original scenarios with the original rules, reading maps, unit/item/spell data and art from
**your own installed copy** at runtime. Without an install it runs a small built-in demo made of
our own content.

Branch: `dt-revival` (16 commits on top of `main`; `main` is untouched). About 22 k lines of Rust.

## How to play

```sh
export RAZDOR_DT_DIR="/path/to/Discord Times Community Update"   # folder with Maps_Rus/, Rus_*.ini, Graphics/
cargo run --release
```

Pick a scenario (the built-in demo or any map in `Maps_Rus`), then a hero class from that
scenario's presets. Unset `RAZDOR_DT_DIR` to play the demo only.

Controls: click the map to walk (a dotted route shows the travel time); right click / Space
stops; wheel or +/− zooms; 1 / 4 wait 1 or 4 hours; M minimap; J journal; B spell book;
N music off/on; Esc menu (save, load, main menu, music and sound volume). In battle: click a framed card to attack or cast (right click picks
the other action), a lit cell to step there, Space to end the unit's turn.

## What is in

| Area | State |
|---|---|
| **Original files** | `.DTm` scenarios (all 15 shipped maps parse byte-exactly and re-serialise identically), `Rus_Units/Artefacts/Spells.ini`, `_Global.ini`, art (`.ugs`, `.lit`, `.spi`; all 421 files decode, pixel-identical to the reference decoder). |
| **World map** | Scenario terrain, trees, hills, mountains, buildings and army figures drawn with the original art; 8-way square grid (see *Decisions*); A* routes with travel time; army and building tooltips (army formation preview); zoom. |
| **Time** | The original calendar (30-day months, days from 0, hours). Time passes while walking, waiting, healing, casting. Villages refill at 00:00; the income/wages report and an autosave come at 12:00 (as in the footage). |
| **Fog of war** | Unexplored land is black with a soft edge and cannot be walked; the hero feels his way into the dark; lanterns and scripted reveals light areas. Minimap of the explored land with owner-coloured icons. |
| **Armies** | Placed from the scenario, active/inactive, factions and attitudes; hostile armies chase and attack; friendly ones greet (events run on meeting). |
| **Buildings** | All 16 types. Building window with the original's tabs by type: main hall (quests, rumours for 10 gold), barracks (stock that regrows, paid healing, resurrection within 7 days), garrison, market + sell shop (25% sale price, prices by attitude), sanctuary (learn spells), village tribute (gold and mana; priest heal; innkeeper pays the unpaid). Capturing forts and castles changes owner and income. Ruins give their treasure. |
| **Economy** | Gold and mana; building income; wages from unit cost (recruit / mercenary kinds); unpaid units sit out battles and desert after 7 days; loot = loser's gold ÷ 2 (min 25) plus items; mana from surrendered units. |
| **Battle** | The original's rules: 2×6 formation (Community wide row, as in your install; vanilla 3×4 + reserve supported), deterministic attack − defence (min 1), separate melee/ranged defence, back row +5 vs shots, the real reach rules (front / diagonals at normal damage; long strike halving defence when all three are empty), shooters and mages by row, magic by school with protection % and creature nature, magic power drain, all 21 vanilla unit bonuses and the 31 Community ones (Splash, Flying, Bastion, FateGift, …; see mechanics.md §8), initiative with attacker +1, actions per unit, 25-turn limit, no retreat, hero survives while any unit lives. Cards show the original's stats; hover previews damage or curse effects. |
| **Units** | XP and levels (`StartExpirience·(LevelMultipler/100)^L`, per-level stat gains), promotions along the upgrade tree, 4 item slots with the one-weapon / one-per-type / class rules, `f-`/`d-`/`p-` modifiers, potions, 40-slot backpack, hero class bonuses (knight −10% physical damage to his army, archmage cheaper faster spells, ranger faster and better healing). |
| **Events and quests** | The scenario script engine: global / local / quest / rumour events, time windows and repeats, relative and chained events, all condition and result groups, flags (`%+X -X =X =/X`), yes/no questions, journal, victory and defeat events. The Community extensions: event opcodes 1–20 (editing other events, AI armies' items/units/speed/groups/spells/XP, spell checks, campaign branches with `Game::next_map()`, random flags, AI targets, teleports) and lifting a spell. Story dialogs with pictures and rewards. On РК1 the opening dialog, the first quest and the journal work. |
| **Spells** | Spell book on the world map; cast on your army or a nearby hostile army for mana and game time; effects last into battles; archmage and Caster discounts; scripted spells use the same path. |
| **Sounds and music** | `_Sounds.ini` and `Sounds/` read at runtime (`.wav` as is, headerless `.raw` wrapped in a WAV header in memory, 22050 Hz, `RAZDOR_MUSIC_RATE` to override). Menu theme; the seven map themes shuffled; battle themes; triumph after a won battle and at victory; defeat. Effects for buttons, windows, the battle horn, every battle action (cannon by `ShotWeaponRange`), card moves, event chords, level-ups, spells good/evil, items by type, gold. N mutes the music; volumes and mutes in the Esc menu, kept in `audio.json`. |
| **Saves** | Manual saves and autosaves (before every battle, at every noon; newest 10 kept) in `~/.local/share/razdor/saves` (or `RAZDOR_SAVE_DIR`). A save refers to the map by name + hash and re-reads it from your install. |

Tests: **305 library + 16 app tests** pass with and without `RAZDOR_DT_DIR`; tests on the real
files run only when it is set. `cargo clippy --all-targets` is clean.

## Decisions I made on my own

- **2×6 formation by default**, because your Community Update install and the video use it; the
  vanilla 3×4 with a reserve row is a supported option.
- **Square cells with 8 neighbours for scenarios**, not hexes: РК3's bridges are diagonal chains
  of 1×1 pieces that no hex row parity connects (one is the only way to the capital), and the
  editor's grid is square. The demo keeps its hex map.
- **Daily report at 12:00**, villages at 00:00: the footage shows the report and autosave at noon.
- **Prices fitted to the footage** (+15% per attitude step below 1; a fort capture pays one day
  of income; a beaten unit gives its `Surrender` value in mana).
- Everything the original leaves unknown is marked *(guess)* in the code and listed in
  `docs/reference/mechanics.md` §8 (terrain costs, buff duration of 3 turns, sight radius 7.5
  cells, backpack of 40, spell book of 15, reserve moves, level numbering, …).

Closing the window used to end in a segmentation fault (a native library's exit handler,
after `main` returned); the game now ends the process directly once everything is written.

## Not done yet

- **Ships** (shipyards, pirates): ДС1, ДС2, РК7 and parts of the tutorials are only partly
  walkable without them.
- **AI lords' economy** (hiring, shopping, taking back forts), AI-vs-AI battles, army respawn.
- **Community bonuses and opcodes** are in, but no shipped unit, item or map uses them, so
  they are tested on made-up data only. `Dominate` is undocumented (a guess), several sizes
  are guesses (mechanics.md §8). Not wired outside the rules: the "Next map" button on the
  victory screen, `NoHeal` blocking the world's healing, opcode 8 on the hero's own speed,
  `set_in_building` for garrisons without extra defence (one line in `game.rs`). AI units
  carry no own items or spells, so opcodes 6 and 11 act on whole armies.
- Village alternatives beyond healing and paying the unpaid (blessing, furs, magic ritual).
- The hero preset's start building; `#HERONAME` shows the class name (no name entry yet).
- Soft terrain transitions, a map editor.
- Sounds: the menu bells (`MainMenuSelect-*`), the scroll sound and `BkgAuthors` (no
  credits screen) are not used yet. **Nobody has listened yet**: the sound was checked by
  logs, a decode round trip and quad-snd loading every file at volume 0.
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
# a core dump from an offscreen snapshot run, if systemd kept one
coredumpctl list razdor
```

Keep the scratch folder if you want the research notes (`RULES.md`, `DTM_FORMAT.md`, the Python
decoders); the cleaned versions of the notes are already in `docs/reference/`.

## Next steps I'd suggest

1. Play РК1 end to end and report problems.
2. Ships, then AI lords' economy.
3. Wire the campaign "Next map" into the victory screen.
4. Merge `dt-revival` into `main` once you're happy, and publish for the community.
