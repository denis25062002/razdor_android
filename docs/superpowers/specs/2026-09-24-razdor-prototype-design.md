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
Pick a hero class. As in the original, the three classes are unit ids 1–3; their stats are
our own numbers in `data/units.ini`:

| Class    | Hits | Attack               | Defence (melee/ranged) | Init | Actions | Gold | Class bonus |
|----------|------|----------------------|------------------------|------|---------|------|-------------|
| Knight   | 70   | melee 24             | 10 / 8                 | 8    | 1       | 100  | his army takes 10% less physical damage |
| Archmage | 42   | Elemental magic 26   | 2 / 4                  | 22   | 2       | 120  | curses slow the enemy (fewer actions, less initiative) |
| Ranger   | 55   | ranged 17, regen 5%  | 4 / 6                  | 16   | 2       | 110  | the army heals 20% of max HP each day |

### World map and time (v2, as in the original)
Replaces the v1 four-node graph. Sources: overview image of the first official map on the
Discord Times wiki, mygames.org.ru, pooha.net tips, discordtimes.ucoz.ru walkthrough.

**Map.** A hand-made 64×44 **hex** map (pointy-top hexes, odd rows shifted half a hex right,
one character per hex) stored as text in `data/kingdom.txt`, laid out like the
original's first scenario: castle in the north-west, villages, forests crossed by dirt roads,
a ruined fortress, a church, a fishing village by a lake, swamp in the south-west, sea along
the south and a second castle with a bridge in the south-east.

| Terrain  | Char | Game time per hex  | Passable |
|----------|------|--------------------|----------|
| Road     | `=`  | 30 min             | yes      |
| Grass    | `.`  | 1 h                | yes      |
| Forest   | `T`  | 2 h 30             | yes      |
| Swamp    | `,`  | 3 h                | yes      |
| Water    | `~`  | —                  | no       |
| Mountain | `^`  | —                  | no       |

Location letters sit on road hexes (listed in `World::standard`).
Click a hex and the party walks the cheapest path (A* over the 6 hex neighbours). The camera
follows. Positions use world units where neighbouring hex centres are 1 apart.

**Time.** A clock (day, weekday, HH:MM), starting day 1 (Monday) 08:00. **Time passes only
while the party moves** (as in the original): walking a hex costs its terrain time; real
speed is 1 game hour ≈ 0.2 s. At **00:00** a new day starts *(sources disagree between noon
and midnight; midnight chosen, it matches the "wait for 24:00 to collect tribute" tip)*:
- castle income is added (Oakford: 20 gold);
- every recruit is paid its daily wage (hero is free); if gold runs out, the remaining
  units are **unpaid** and refuse to fight (they sit out battles) until a later payday;
- village tribute becomes available again; camps may send out a new gang.

**Locations.**
- **Castle** (Oakford: yours, income 20/day; Greywall: foreign). Recruit, full heal on entry.
- **Village** ×3: once per day either collect tribute (10 gold) or ask the priest to heal
  the squad.
- **Church**: full heal, free.
- **Bandit camp** / **Bandit lair**: garrisoned; entering starts the battle. Clearing both wins.

**Roaming parties.** Bandit gangs move on the map in the same game time as the player
(0.8× player speed on equal terrain). They wander around their home camp, and chase the
player when within 5 hexes. Touching a gang starts a battle against its formation (reward
30 gold). After a stalemate the gang ignores the player for 2 game hours. Each uncleared camp
spawns a gang every 3 days while it has fewer than 2 out. Two gangs roam from the start.

Wages follow the original's formula from the unit's cost (mechanics.md 1.5):
`round(Cost / 2 × f)` with f = ¼ up to 50 gold, ½ up to 100, ¾ up to 150, 1 above.
`AddPayment` in the army cuts wages by 30%.

| Recruit   | Cost | Wage/day |
|-----------|------|----------|
| Spearman  | 40   | 5        |
| Archer    | 45   | 6        |
| Swordsman | 50   | 6        |
| Healer    | 50   | 6        |

### Units and content (v5: data-driven)
All rules read a `rules::content::Content`: unit, item and spell definitions plus the
`_Global.ini` options (magic divisors, drain and floors, `Row2Def`, `BattleEndTurn`, XP and
wage constants, sale price). It comes either from a Discord Times install
(`Content::from_dt`) or from the built-in demo (`Content::builtin`): `data/units.ini` and
`data/items.ini`, our own content written in the original's ini schema, so the same parser
reads both. Demo-only keys: `Key=` (sprite file stem, stable name), `StartGold=`, `Sources=`.
The demo options are the vanilla defaults except faster XP (player XP 100% instead of 50%,
`MainExpCorrection` 60).

| Unit          | Cost | Hits | Attack         | Defence | Init | Notes |
|---------------|------|------|----------------|---------|------|-------|
| Spearman      | 40   | 40   | melee 16       | 6 / 3   | 7    | `SpearDefense`; promotes to Swordsman at level 2 |
| Archer        | 45   | 30   | ranged 14      | 2 / 4   | 12   | |
| Swordsman     | 50   | 50   | melee 20       | 9 / 6   | 8    | |
| Healer        | 50   | 28   | Life magic 14  | 1 / 2   | 10   | heals the wounded, blesses the rest |
| Bandit        | —    | 36   | melee 15       | 4 / 2   | 7    | |
| Bandit archer | —    | 26   | ranged 12      | 1 / 3   | 11   | |
| Bandit chief  | —    | 80   | melee 22       | 8 / 6   | 9    | `Counterblow` |

A unit's role follows from its stats (melee attack → warrior, ranged → shooter, magic power
and a school → mage). Units have a **level** (1 as hired) and **XP**; the next level needs
`StartExpirience × (LevelMultipler/100)^(level−1)` and adds the type's `d-*` gains. A unit
that reaches `NextUnitNLevel` can be **promoted** to `NextUnitN` from the Squad screen: it
starts the new class at level 1 *(guess)*; items it can no longer wear go to the pack.
Recruits that die in battle are gone.

Camp formations — Bandit camp: 3 bandits front, 2 archers back, reward 100.
Bandit lair: chief + 2 bandits front, 2 archers back, reward 150.

### Battle (v5: the original's rules)
Replaces the v3 wiki-based rules. Full rules and formulas: `docs/reference/mechanics.md`
sections 2 and 3.3; our guesses are listed in its "Razdor implementation choices" section.

- **Formation** from `Content`: default **2 × 6** (the Community wide row), vanilla
  **3 × 4 with a reserve row** supported. The reserve cannot act (except to step out) and
  cannot be targeted. Deploy phase first; the deployed formation is kept afterwards.
- **Turn order:** by initiative, the attacker +1 (walking into a camp makes you the attacker,
  a gang that catches you attacks), `Artillery` first. Each unit has `Manevres` actions per
  turn (an attack, a spell or a step); `HorseAtack`/`OldVampirsGist`/`FastDead` +1 on turn 1.
- **Moves:** to an empty own cell in columns c−1..c+1 of the front and back rows; from the
  reserve to any front/back cell; into the reserve from the back row *(guess)*.
- **Reach:** warriors from the front row hit enemy front cells c−1..c+1; with all three
  empty, a **long strike** reaches the nearest front unit on either side, halving its
  defence (`FlankStrike` doubles the attack). Shooters and hostile mages in the back row
  reach anyone in the enemy's front and back rows; in the front row, shooters only the
  adjacent front units unless none are there, mages nothing unless none are there. Friendly
  mages heal the wounded or bless the rest.
- **Damage** is deterministic: attack − defence, at least 1, with the vanilla bonuses
  (`SpearDefense`, `ArmorIgnore`, `Unvulnerabe`, `Ghost`, `Evasive`, `Dead`, `GodAnger` …),
  +`Row2Def` defence against shots in the back row, knight hero −10%.
- **Magic** by school with protection % and nature multipliers; blessings and curses last
  3 turns *(guess)*; mage power drains each turn down to a floor.
- **Collapse:** when a front row is empty the back row steps forward (then the reserve).
- **End:** a side with no living units loses. The hero cannot die while his army lives: he
  comes back with 1 HP. After `BattleEndTurn` (25) turns the battle is a stalemate: you
  withdraw, the camp stays. There is no retreat.
- **XP** after the battle for the survivors, from the strength of the enemy destroyed
  (mechanics.md 1.4, simplified); shown as "XP +N" on the cards.
- **AI:** heal a badly hurt ally, else kill if it can, else (non-Life casters) curse an
  uncursed enemy, else the attack needing the fewest hits; warriors in the back row step
  into the front row.
- **UI:** cards show the original's strip (`A:`/`S:`/`Pwr:`, `D: melee/ranged`, `Mnvr`,
  `Ini`, `Hits`, level); lowered values red, raised blue. Hovering a target previews
  "strike: −N hits" or the curse's effect; right click picks the alternative action (e.g.
  a mage's strike instead of its curse).

### Items and market (v5, as in the original)
Items are artefact definitions (`data/items.ini` in the demo, `Rus_Artefacts.ini` from an
install): type, price, `d-`/`p-`/`f-` stat modifiers, a granted bonus or magic school.

**Slots.** Every unit has 4 slots: only **one weapon** (melee weapon, bow or staff), never
two of the same type; melee weapons need a warrior, bows a shooter, staffs a mage. Potions
and trade goods cannot be worn. Modifiers apply `f-` first (sets the stat), then all `d-`,
then the summed `p-` percentages *(guess: the original's order is unknown)*.

**Potions** are drunk from the Squad screen: healing is instant, other modifiers last until
the end of the next battle.

**Pack.** The party shares a pack of up to 16 unworn items *(guess)*. Loot, tribute and
purchases go there; what does not fit is left behind. Taking off an item that raised max HP
caps current HP to the new max. A recruit who dies loses its gear.

**Market.** Both castles have one: 6 items drawn at random from `market` items, restocked
every Monday 00:00. Selling pays `ItemSaleCost` = 25% of the price; a `Merchant` in the
army gets +50% when selling and −30% when buying. Personal (negative-price) items cannot be
sold.

**Loot.** Bandit camp victory: 1 item; lair: 2; a beaten gang drops one 30% of the time.
Drawn from `loot` items. **Tribute:** a village pays a `tribute` item instead of gold 25% of
the time (gold if the pack is full).

### Out of scope (YAGNI)
Quests, dialogue, save/load, sound, morale, fog of war.
Items leave out: item durability, enemy gear, "dark forces only" items, the AI using potions.
Map v2 leaves out: ships (half-speed time), resurrecting the dead within 7 days, leaving
garrisons, capturing castles, taverns, non-bandit parties (peasants, undead, feudal lords).

## Architecture

```
src/
  main.rs          screen state machine + macroquad loop
  rules/           pure game logic, no macroquad dependency, unit-tested
    mod.rs
    rng.rs         small seeded xorshift RNG (deterministic tests)
    content.rs     Content: unit/item/spell defs and options, from an install or data/*.ini
    units.rs       Stats (by stat, bonuses, magic), Unit (level, XP, HP, items, promotion)
    items.rs       slot and class rules, f-/d-/p- modifiers, potions, sale price
    map.rs         TileMap: hex terrain grid parsed from data/kingdom.txt, hex math, A*
    clock.rs       Clock: game minutes → day / weekday / HH:MM, midnight rollover
    world.rs       Locations and roaming parties on the tile map
    formation.rs   Row, Slot, Formation (2×6 or 3×4 + reserve)
    battle.rs      Battle: deploy, turn order, reach, damage, magic, effects, XP, AI
    game.rs        Game: hero, gold, squad, clock, travel tick (movement, time, parties,
                   encounters, paydays), hiring, tribute, pack, equip, market, loot,
                   battle setup/resolve
  ui/              macroquad presentation only
    mod.rs
    assets.rs      Assets: draws units/tiles; optional PNG override dir
    widgets.rs     button + text helpers
    screens.rs     class select, town, end screens
    world_view.rs  hex world map
    battle_view.rs card battle
    items_view.rs  squad/gear and market screens
```

- `rules` never imports macroquad; the UI reads rules state and calls rules methods.
  That keeps logic testable with `cargo test` and lets the UI layer be swapped
  (e.g. to Bevy) without rewriting the rules.
- **Assets layer:** every unit/tile is drawn through `Assets`. Default is
  coloured shapes + a letter. If env var `RAZDOR_ASSETS` points to a directory,
  `<unit_name>.png` files there override the shapes. That directory is never committed.
- Enemy turns are played with a short delay and a strike/hit animation so they are readable.

## Testing
Unit tests in `rules` with small synthetic `Content` (no game files): every reach rule,
long strike and `FlankStrike`, `Row2Def` vs shots only, `SpearDefense` on turn 1,
`ArmorIgnore`, `Unvulnerabe`/`Ghost`, `Evasive`/`Dead`/`GodAnger`/Evasion, knight −10%,
counterblow and death curse, magic by school and nature with protection, blessings and
curses, power drain to the floor, reserve rules, collapse, movement, initiative with the
attacker bonus, actions, turn limit, hero survival, XP split, AI choices; XP-to-level,
level-up deltas, promotion; item slot/class rules and modifier order; wages; the demo
data; map, clock, world and game flow. With `RAZDOR_DT_DIR` set, `Content::from_dt` loads
the install and auto-played battles between armies of real units always end (numeric
assertions only). UI is verified by running the game.
