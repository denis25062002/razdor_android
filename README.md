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
- Warriors fight only from the front row and hit the card opposite them. If that cell is
  empty they can hit a neighbouring column instead: a flank strike, double attack.
  When a side's front row falls, its rear steps forward.
- Shooters and mages hit anyone; magic ignores armor. Armor always lets 1 damage through.
  No counterattacks, as in the original.
- After 20 rounds an undecided battle ends and you withdraw. Squad cap: 12.
- Clear both bandit camps to win. If your hero dies, it's over.

## Custom sprites
All art is placeholder tokens. To use your own, put PNGs named after units
(`knight.png`, `archmage.png`, `ranger.png`, `spearman.png`, `archer.png`, `swordsman.png`,
`healer.png`, `bandit.png`, `bandit_archer.png`, `bandit_chief.png`) in a folder and run:

```sh
RAZDOR_ASSETS=./assets-local cargo run --release
```

`assets-local/` is git-ignored — keep third-party art there.

## Layout
- `src/rules/` — pure game logic (no macroquad), unit-tested.
- `src/ui/` — macroquad screens; `assets.rs` is the only place that draws units.
- Design: `docs/superpowers/specs/2026-09-24-razdor-prototype-design.md`.
