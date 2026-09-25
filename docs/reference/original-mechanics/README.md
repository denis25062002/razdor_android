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

Each file ends with a **"Razdor now → original"** table listing where the engine still
differs. Those tables are the work list for bringing Razdor in line.

Not covered here yet: the experience system (next investigation), and the file formats, which
are in [../dtm-format.md](../dtm-format.md) and [../graphics-formats.md](../graphics-formats.md).
The older overview with the data-file semantics is [../mechanics.md](../mechanics.md).
