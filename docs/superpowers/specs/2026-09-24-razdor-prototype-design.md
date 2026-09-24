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

Recruits that die in battle are gone.

Enemies: Bandit (26 HP, 5–8, armor 1, melee), Bandit archer (18 HP, 4–7, ranged 6),
Bandit chief (55 HP, 9–13, armor 3, melee).

### Battle (v3: card formation, rules from the Discord Times wiki)
Replaces the v1 10×8 movement grid. Rules follow the fan wiki
(discorttimes.fandom.com: Параметры, Фланговый удар, Стрелок); gaps are marked *(guess)*.

- Each side has a **2×6 formation**: a front row and a back row of 6 cells. Before the
  fight there is a **deploy phase** where the player moves/swaps their own cards; the
  formation is kept for the next battle.
- Every unit is a **warrior** (melee), **shooter** (ranged) or **mage** (magic / heal).
- **Actions:** on its turn a unit has `actions` points (Ranger 2, others 1). Each attack,
  heal or move costs 1. Space ends the turn early.
- **Move:** step to an orthogonally adjacent empty cell of your own formation *(guess:
  orthogonal only)*.
- **Warrior reach:** only from the front row (back-row warriors are helpless). Hits the
  enemy front-row card in the same column. If that opposite cell is empty it may instead
  hit a card in a neighbouring column: a **flank strike**, attack ×2.
- **Collapse:** when a side's front row is empty, its back row steps forward (same columns).
  So melee never reaches the back row directly.
- **Shooter / mage:** may target any enemy. Magic ignores armor.
- **Healer:** restores HP to any wounded ally, capped at max HP.
- Damage = roll(min..=max) (×2 on a flank) − armor (magic ignores armor), **minimum 1**:
  armor never fully blocks.
- **No counterattacks** (none documented in the original).
- Turn order each round: living units by initiative (desc), ties → player first.
- Enemy AI, per action: act on the lowest-HP legal target (flank preferred on ties); a
  warrior with no target steps toward a cell that has one; otherwise it waits.
- End: all enemies dead = victory; hero dead = defeat. After **20 rounds** the battle
  stops undecided: the squad withdraws, the camp stays, no reward.
- Squad cap: 12 including the hero. Hiring drops a unit into the first free cell of
  its preferred row (warriors front, others back), centre columns first.

Unit changes vs v1: `moves` and ranges removed; `actions` added (Ranger: 2, dmg 5–7).
Camp formations — Bandit camp: 3 bandits front, 2 archers back, reward 100.
Bandit lair: chief + 2 bandits front, 2 archers back, reward 150.
Knight armor 5. Enemy stats retuned: Bandit 28 HP 6–9, Bandit archer 20 HP 5–8, Chief 65 HP 11–15 armor 3.
Balance (AI vs AI, 200 seeds): hero alone loses the camp; hero + 3 spearmen wins it;
the lair needs ~5 recruits (hero + 3 spearmen alone loses it).

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
    formation.rs   Row, Slot, 2×6 formation helpers
    battle.rs      Battle: deploy, turn order, attack/heal, win/lose/stalemate, AI
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
- Enemy turns are played with a short delay and a strike/hit animation so they are readable.

## Testing
Unit tests in `rules`: turn order, column reach and flank ×2, helpless back-row
warriors, collapse, movement and action points, armor minimum 1, magic, heal cap, deploy
swaps, win/lose/stalemate, AI targeting and approach, travel, hiring, battle results.
UI is verified by running the game.
