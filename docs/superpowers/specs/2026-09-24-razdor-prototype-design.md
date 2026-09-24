# Razdor — prototype design

A small Rust prototype inspired by *Discord Times* («Времена раздора», Aterdux, 2004):
a wandering noble travels a kingdom map, hires a squad in towns and fights
turn-based tactical battles on a grid.

## Goal

A thin, playable end-to-end vertical slice that proves the core loop
(travel → hire → fight → reward) works and feels right. Not a clone, not polished.
No original game assets are used; all art is placeholder shapes, swappable later.

## Scope

### Start
Pick a hero class:

| Class    | HP | Dmg   | Armor | Move | Init | Attack              | Gold |
|----------|----|-------|-------|------|------|---------------------|------|
| Knight   | 60 | 10–14 | 4     | 3    | 5    | Melee               | 100  |
| Archmage | 32 | 9–13  | 0     | 3    | 6    | Ranged 6, magic     | 120  |
| Ranger   | 40 | 8–11  | 1     | 4    | 7    | Ranged 7            | 110  |

### World map
Four hand-placed locations joined by roads:

- **Oakford** (home town) — recruits: Spearman, Archer, Healer
- **Bandit camp** — battle vs 3 bandits + 1 bandit archer, reward 80 gold
- **Greywall** (second town) — recruits: Swordsman, Archer, Healer
- **Bandit lair** — battle vs chief + 2 bandits + 2 bandit archers, reward 150 gold

Roads: Oakford–Camp, Oakford–Greywall, Camp–Lair, Greywall–Lair.
Clicking an adjacent location moves the hero there; each move = 1 day.
Entering a town fully heals the squad. Entering an uncleared camp starts a battle.
Clearing both camps = victory.

### Recruits

| Unit      | Cost | HP | Dmg  | Armor | Move | Init | Attack    |
|-----------|------|----|------|-------|------|------|-----------|
| Spearman  | 30   | 30 | 5–8  | 2     | 3    | 4    | Melee     |
| Archer    | 40   | 22 | 5–8  | 0     | 3    | 5    | Ranged 6  |
| Swordsman | 50   | 38 | 7–10 | 3     | 3    | 5    | Melee     |
| Healer    | 45   | 20 | —    | 0     | 3    | 3    | Heal 10, range 4 |

Squad cap: hero + 4. Recruits that die in battle are gone.

Enemies: Bandit (26 HP, 5–8, armor 1, melee), Bandit archer (18 HP, 4–7, ranged 6),
Bandit chief (55 HP, 9–13, armor 3, melee).

### Battle
- 10×8 square grid. Player deploys in the left two columns, enemies in the right two.
- Turn order each round: all living units sorted by initiative (desc), ties → player first.
- A unit's turn: optionally move (BFS through free cells, up to Move steps),
  then optionally act. Acting ends the turn. Skipping (Space) ends the turn.
- Melee: target an orthogonally/diagonally adjacent enemy (Chebyshev distance 1).
- Ranged: target an enemy within range (Chebyshev). Damage halved if the
  shooter has an adjacent enemy.
- Damage = roll(min..=max) − target armor (magic ignores armor), minimum 1.
- Heal: restore HP to an ally in range, capped at max HP.
- Enemy AI: attack the lowest-HP target reachable this turn; otherwise move
  along the shortest path toward the nearest enemy.
- Win: all enemies dead. Lose: hero dies → game over → back to class select.

### Out of scope (YAGNI)
XP/levels, quests, dialogue, equipment, save/load, sound, morale, fog of war, hex grid.

## Architecture

```
src/
  main.rs          screen state machine + macroquad loop
  rules/           pure game logic, no macroquad dependency, unit-tested
    mod.rs
    rng.rs         small seeded xorshift RNG (deterministic tests)
    units.rs       UnitKind, Stats, AttackKind, Unit
    world.rs       World map: locations, roads, travel
    battle.rs      Battle: grid, turn order, move/attack/heal, win/lose, AI
    game.rs        Game: hero, gold, squad, day, location; hire; battle setup/resolve
  ui/              macroquad presentation only
    mod.rs
    assets.rs      Assets: draws units/tiles; optional PNG override dir
    widgets.rs     button + text helpers
    screens.rs     class select, world map, town, battle, end screens
```

- `rules` never imports macroquad; the UI reads rules state and calls rules methods.
  That keeps logic testable with `cargo test` and lets the UI layer be swapped
  (e.g. to Bevy) without rewriting the rules.
- **Assets layer:** every unit/tile is drawn through `Assets`. Default is
  coloured shapes + a letter. If env var `RAZDOR_ASSETS` points to a directory,
  `<unit_name>.png` files there override the shapes. That directory is never committed.
- Enemy turns are played with a short delay (~0.35 s) so they are readable.

## Testing
Unit tests in `rules`: turn order, movement range/blocking, damage/armor/magic,
ranged adjacency penalty, heal cap, win/lose detection, AI picks a target and
approaches, travel adjacency, hiring cost/cap, battle result updates the squad.
UI is verified by running the game.
