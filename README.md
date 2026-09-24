# Razdor

A small Rust prototype inspired by *Discord Times* («Времена раздора», Aterdux, 2004):
travel a kingdom map, hire a squad in towns, fight turn-based tactical battles on a grid.

```sh
cargo run --release
cargo test          # game rules
```

## How to play
- Pick a hero: Knight (melee tank), Archmage (magic ignores armor), Ranger (long bow).
- The kingdom is a hex map, as in the original (`data/kingdom.txt`, one character per hex,
  odd rows shifted half a hex): click anywhere to walk the cheapest route. Roads are fast, forest and swamp slow, water and mountains
  impassable. Right click or Space stops.
- Time runs only while you travel, as in the original. At midnight your castle pays income
  and every recruit takes a daily wage; units you can't pay refuse to fight.
- Villages give tribute once a day, or their priest heals you instead. Castles and the
  church heal the squad; castles recruit.
- Bandit gangs roam the map, chase you when you're close ("!") and attack on contact.
  Surviving camps send out new gangs every few days.
- Battles are fought card-style, as in the original: each side stands in a 2×6
  formation (front row + back row). Before the fight, click a card and then a cell to
  move or swap it, then press **Fight!** (or Enter). The formation is kept.
- Units act in initiative order. The gold-framed card acts. Each attack, heal or step
  costs one action (the Ranger has 2): click a red-framed enemy to attack (green frame =
  heal an ally, "x2" = flank strike), a lit cell to step there, Space to end the turn.
- Warriors fight only from the front row and hit an enemy front-row card straight ahead,
  front-left or front-right. A diagonal hit while the cell ahead is empty is a flank
  strike, double attack. When a side's front row falls, its rear steps forward.
- Shooters and mages hit anyone; magic ignores armor. Armor always lets 1 damage through.
  No counterattacks, as in the original.
- After 20 rounds an undecided battle ends and you withdraw. Squad cap: 12.
- Items, as in the original: every unit has 4 slots (one item per type, potions may
  stack). Buy them at castle markets (new stock every Monday, sell for half price), loot
  them from camps and gangs, or get them as village tribute. Manage gear from
  **Squad & gear** on the map or in a castle. In battle, click a potion button to drink
  it (one action). Items are defined in `data/items.txt`.
- Clear both bandit camps to win. If your hero dies, it's over.

## Using your Discord Times install
Razdor is becoming an engine for the original game's scenarios. It reads the data from **your
own installed copy** of *Discord Times* (Community Update) at runtime; the repo contains no
original maps, data, text or art, and nothing from your install is ever copied or written.

```sh
export RAZDOR_DT_DIR="/path/to/Discord Times"   # the folder with DiscordTimes.exe
cargo test                                      # also checks the readers against your files
```

What is read (only read, never modified): `Rus_Units.ini`, `Rus_Artefacts.ini`,
`Rus_Spells.ini`, `_Global.ini` and the scenario maps `Maps_Rus/*.DTm`. The readers live in
`src/dt/` (`dt::install::DtInstall::from_env()`); the formats are described in
`docs/reference/`. Without the variable everything still works, and the tests that need the
real files are skipped.

## Custom sprites
All art is placeholder tokens. To use your own, put PNGs named after units
(`knight.png`, `archmage.png`, `ranger.png`, `spearman.png`, `archer.png`, `swordsman.png`,
`healer.png`, `bandit.png`, `bandit_archer.png`, `bandit_chief.png`) and items (by their id
in `data/items.txt`, e.g. `short_sword.png`) in a folder and run:

```sh
RAZDOR_ASSETS=./assets-local cargo run --release
```

`assets-local/` is git-ignored — keep third-party art there.

## Layout
- `src/dt/` — readers for the original's files (ini data, `.DTm` maps). Pure, no macroquad.
- `src/rules/` — pure game logic (no macroquad), unit-tested.
- `src/ui/` — macroquad screens; `assets.rs` is the only place that draws units and items.
- Design: `docs/superpowers/specs/2026-09-24-razdor-prototype-design.md`.
