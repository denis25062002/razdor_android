# Razdor

A small Rust prototype inspired by *Discord Times* («Времена раздора», Aterdux, 2004):
travel a kingdom map, hire a squad in towns, fight turn-based tactical battles on a grid.

```sh
cargo run --release
cargo test          # game rules
```

## How to play
- Pick a hero: Knight (melee tank), Archmage (magic ignores armor), Ranger (long bow).
- On the map, click a highlighted neighbouring location to travel (1 day).
  Towns heal your squad and sell recruits; red triangles are bandit camps.
- Battles are fought card-style, as in the original: each side stands in a 2×6
  formation (front row + back row). Before the fight, click a card and then a cell to
  move or swap it, then press **Fight!** (or Enter). The formation is kept.
- Units act in initiative order. The gold-framed card acts: click a red-framed enemy to
  attack (green frame = heal an ally), Space to wait.
- Warriors hit the enemy front row, and the back row only once the front is empty.
  A warrior in your back row can't act while your front row stands.
  Shooters and mages hit anyone; magic ignores armor. Heavy armor can block a hit.
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
