# How the original Discord Times works

A specification of the original game's rules, read from the Community Update (Unstable)
`DiscordTimes.exe` (Delphi) of the player's own install, cross-checked with its data files, its
help and editor manual, and gameplay footage. Written in our own words for interoperability:
formulas, tables and prose only. No code, disassembly or game text is reproduced. Addresses
are virtual addresses in that build, given as evidence so a claim can be rechecked.

Confidence tags used throughout:
- **code** / **H**: confirmed by reading the code.
- **data** / **M**: consistent with the data files, help or footage, or partly traced.
- **unknown**: not determined; Razdor keeps a documented guess.

| File | Covers |
|---|---|
| [battle.md](battle.md) | Battle model, formation (vanilla 3×4 + reserve; wide row = front 6, back 4, reserve 2), damage formula, buff/curse duration and stacking, magic drain, reserve moves and collapse, mage action choice, battle AI, turn limit and surrender, all Community bonuses, turn order, poison/regen/vampirism, hero survival |
| [world.md](world.md) | Square 8-neighbour grid, terrain and object movement costs, footprints, speeds, real-time pacing, waits, sight radii and fog, contact, AI view distances and goal choice, day ticks at 00:00 and 12:00, hero start |
| [economy.md](economy.md) | Wages (recruits, mercenaries, garrisons, desertion), market prices by attitude, restocking, barracks growth, healing and resurrection, the difficulty factor, villages and their options, loot and surrender, spells (costs, timing, stacking, duration), item modifiers, the event engine's edge cases, campaign carry-over |
| [experience.md](experience.md) | Unit strength (tactical cost) from stats, side strength, the battle XP pool and shares, the player's and the AI's modifiers and the 5256 cap, levels (XP table, per-level gains, percent stats, HP), promotion for the player and the AI, event, opcode and hiring XP, the hero preset (gold and mana, no XP), carry-over |
| [ai.md](ai.md) | World-map AI: per-frame driver, step clock, relations, the simulated-battle army score, building scores, planner and repulsion, chasing, patrol, fleeing, hiring and promotion, respawn, AI ships, AI-vs-AI fights |
| [magic-items.md](magic-items.md) | Spells and items key by key: book and learning, world casting and its wait, the per-unit spell slots, durations and stacking, the pack and equip screens, the wear test, the stat rebuild order and rounding, potions, AI item use |
| [events.md](events.md) | The scenario scripting engine: scan order, guards and time windows, all conditions, flags, the ask/OK flow, every result in order, chains and delays, meetings, quests and rumours, victory and defeat, campaign carry-over |
| [saves-data.md](saves-data.md) | How the code reads every ini file and key (matching, defaults), map loading, the save and autosave format, options, the in-memory records a reimplementation needs |
| [interface.md](interface.md) | Game flow and every screen: start-up and loading, menus, hero choice, the world screen, mouse and keys, camera, windows, hints, message boxes, battle screen, music and sound triggers, timings, options |
| [community-patches.md](community-patches.md) | Everything the Community Update adds: all 126 hooks into `.mod`, the relocated tables in `.bonus`, every Community unit bonus and ini key, the wide row, and the vanilla behaviour behind each hook |
| [engine.md](engine.md) | Display and frame loop, the clock, the random generator (exact algorithm and call sites), input, text and fonts, colour, animation and sound timing |

Each file ends with a **"Razdor now → original"** table listing where the engine still
differs. Those tables are the work list for bringing Razdor in line.

Not covered here: the file formats, which are in [../dtm-format.md](../dtm-format.md) and
[../graphics-formats.md](../graphics-formats.md).
The older overview with the data-file semantics is [../mechanics.md](../mechanics.md).
