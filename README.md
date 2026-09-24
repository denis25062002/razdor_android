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
- In battle, the unit with the gold ring acts. Click a lit cell to move, a red-framed
  enemy to attack (green frame = heal an ally), Space to end the turn.
- Ranged units deal half damage while an enemy stands next to them.
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
