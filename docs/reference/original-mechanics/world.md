# Discord Times: world map, movement, time and AI (from the user's Community exe)

Rules in plain words. Addresses are VAs in the user's `DiscordTimes.exe` (image base 0x400000);
they point to the code that does it. No code is reproduced. Confidence: **H** high (read
directly), **M** medium (read, but a detail or unit is inferred), **L** low / partial.

Runtime layout used below (for orientation only):
- Army slots: slot 0 = hero, slot k = DTm army k (1-based); 0x3827-byte records at
  `0x75a940 + k*0x3827`. Field offsets below are relative to that record.
- Cell records (18 B) at `[0x68ecb0]`, index `y*(W+8)+x`: +0 massif object (class<<8|sprite,
  classes 1–8), +2 plant object (classes 9–12), +4 building id at its anchor, +6 point id,
  +8 army standing there, +0xC anchor index of the building whose footprint covers it.
- Terrain bytes at `[0x68ecac]`, (W+2)×(H+2) with a copied border.
- Three 16-bit cost maps built at map load (0x4b31dd–0x4b391d): LAND `[0x68e790]`,
  MIXED `[0x68e794]`, SHIP `[0x68e798]`. 0 = impassable.
- Time `[0x68dcb8]` is **centi-minutes** since scenario start; absolute minutes =
  `[0x68dcb8]/100 + [0x68dcbc]` (0x49d631, 0x4abfd6).

---

## 1. Movement cost, objects, grid — H

**Grid: square, 8 neighbours.** Neighbour table dx `0x4ecf8c` = (−1,0,1,1,1,0,−1,−1),
dy `0x4ecfb0` = (−1,−1,−1,0,1,1,1,0) (direction 0 = up-left, clockwise; odd = orthogonal).
Step weight table `0x4ecfd4` = diag 3, orth 2. Screen cell 32×22 px (0x4b2c79, 0x4aea08).
No hex anywhere.

**Cost of a cell** (0x4b3285–0x4b391d), terrain table `0x4ed128`, object table `0x4ed164`:
1. Massif objects of class 1–4 (hills) *set* the cost to the class value over a square.
2. Every cell then adds its terrain value (cell untouched by step 1 starts from 0). Result
   below 0 → 0 (blocked).
3. Terrain code 0–2 is **water**: LAND = 0, SHIP = cost, MIXED = cost. Codes ≥3: LAND = cost,
   MIXED = 5×cost, SHIP = 0.
4. Plant objects (class 9–12) *add* their class value on their own cell (if still passable).
5. Massif objects of class 5–8 add their value (−32 → blocked) over their square.
6. Every building footprint is then overwritten with the **road** value (3) on LAND and SHIP
   (6 on MIXED) — buildings and bridges are walkable at road speed.

Terrain values (cost units; minutes per orthogonal step on foot = value × 5, see §2):

| code | surface | value | on foot | by ship |
|---|---|---|---|---|
| 0 | shallows / fords | 2 | **blocked** | 2 (10 min) |
| 1 | coastal water | 1 | blocked | 1 (5 min) |
| 2 | deep sea | −32 | blocked | **blocked** |
| 3 | lava fields | −32 | **blocked** | blocked |
| 4 | road | 3 | 15 min | – |
| 5,6,7 | grass lowland / plain, dry plain | 5 | 25 min | – |
| 8 | marsh | 8 | 40 min | – |
| 9 | impassable swamp | −32 | blocked | – |
| 10 | sand | 5 | 25 min | – |
| 11 | clay | 5 | 25 min | – |
| 12 | stony soil | 4 | 20 min | – |
| 13 | scorched land | 5 | 25 min | – |
| 14 | snowy ground | 6 | 30 min | – |
| 15 | snowdrifts | (reads −32, the next table's first entry) | blocked | – |

Object class values: 1,2,3 = +2 (hills); 4 = +3; 5,6,7 = blocked (mountains); 8 = blocked
(rocks); 9 = +4 (trees); 10 = +6 (dead/dry trees — slower than trees); 11 = blocked (thicket);
12 = +4. E.g. hill on grass = 7 (35 min), tree on grass = 9 (45 min).

**Massif footprint** (classes 1–8): a square of side `f = sprite div 10` cells whose
**bottom-right** cell is the object's cell (rows y−f+1..y, cols x−f+1..x), clipped to the map.
Plants cover only their own cell.

**Diagonal**: planner weight ×1.5 (3 vs 2) and walking time ×1.5 (0x4ae954, 0x4af263,
0x4cc9cd). Not √2, no pixel aspect.

**Planner** (0x482a58): a wavefront/Dijkstra on the cost bitmap: reaching a neighbour costs
`cost(neighbour) × weight(dir)`; multiple sources allowed; the path is read back by steepest
descent (0x482fe8). Before the flood the cost map is multiplied by a 0/1 mask (0x482b08).

**Hero's mask** (0x4cc46d–0x4cc86b): starts all-open, then closes
- cells of other active armies that *patrol with radius 0* (stationary guards), except the
  clicked army;
- cells within 1 of the hero of armies flagged +0x37e6 (not decoded);
- footprints of castles/forts whose attitude to the player is ≤ 0, and of ruins still owned
  by someone other than the hero — unless it is the clicked building or the one he stands in;
- when at sea, bridge footprints;
- every **unexplored** cell (mask × explored map, 0x4cc857).
Other buildings (towns, villages, churches…) are **not** obstacles: the path may cross them.

**Walking the path** (0x4ae6dc, 0x497c68): stepping onto *any* footprint cell of a non-bridge
building enters it (`0x68dc74`). The step time charged is the cost of the cell being **left**
(recomputed on arrival, 0x4980a8), while the planner prices the cell being entered.

## 2. Speed and real time — H

- Hero step time (centi-min) = `cellcost × speed × 100`, ×1.5 diagonal (0x49809a).
  Speed by class (0x4b4300): **knight 5, archmage 5, ranger 4** → Ranger steps take 80% of the
  time (the "+20%"). Minutes per orthogonal grass step: 25 (ranger 20).
- AI army speed (loader 0x4b4824): `max(1, 5 − speed_correction)` (DTm byte 13), then **−1 if
  the leader unit is GlobalIndex 2 (Архимаг)** — probably meant as the ranger; clamp ≥1. So one
  point of correction = 20% of the base time, not 10%.
- Ships: no separate speed. On water the hero uses the MIXED map (water raw cost): coastal
  5 min, shallows 10 min per step (ranger 4/8). AI ships use the SHIP map (water only)
  (0x4a203b).
- Each hero step (and each wait tick) plays over `WalkDelay = 150 + (100 − WalkSpeed)·2.5`
  ms of real time (0x4b8c54; WalkSpeed=100 in the shipped ini → 150 ms). So real time per step
  is constant; game minutes per real second = step minutes / WalkDelay (≈167 game-min/s on
  grass at default speed, 200 game-min/s while waiting).
- AI clock (0x4a39d0–0x4a3d91): every hero step adds the hero's step time to each AI army's
  budget (cap 200 min); an AI army takes a step when the budget covers its own next step
  (`cost(next cell) × speed × 100 × weight/2`). AI armies only move while the hero walks,
  waits or casts.
- **How AI steps are played** (0x4a39d0–0x4a3e60, runs every frame with the elapsed time):
  at the start of each hero step or wait tick an army's bank gets the tick's time (+0xe124,
  cap 20000 centi-min) and the window is set to that time (+0xe128). An army ready to move
  takes **one** step when the bank covers it; the step's play time (+0xc058) is its cost
  scaled by `tick / bank` if the bank also covers the step after it, otherwise the whole
  rest of the window; never more than what is left of the window. The frame's time counts
  it down; when it runs out the army reaches the cell and may take the next step. So several
  steps in one hero step share the window evenly by cost, and every army walks at the same
  time as the hero (Razdor: `world::Walk`, `Game::army_display_pos`).

## 3. Sight and fog — H (shape), M (exact edge)

- Sight radius per class (0x4b4311): **knight 18, archmage 16, ranger 20 half-cells**
  (= 9 / 8 / 10 cells). Revealed after every step (0x4ae823) and at start (0x4b5954).
- Stamps (0x4cf734): a Euclidean disc in **cell units** (a circle in cells, an ellipse on
  screen). Brightness per half-cell = 15 − 8·(d − r), clamped; a cell becomes explored when the
  average of its four half-cells is bright enough, i.e. roughly within `r/2 + 0.6` cells.
  Maximum r = 48 half-cells.
- Lanterns: start lanterns pass `radius × 2` half-cells (0x4b5a37), so the DTm radius (≤24) is
  in **cells**, same disc. Event lanterns grow from 0 to their radius over time
  (2 half-cells per 50 ms, 0x4af8b3).
- Unexplored cells are **impassable to the hero's planner** (mask, 0x4cc857). The AI's
  planner does not use the explored map (AI ignores fog) — M.

## 4. Contact, view and patrol — H

- Distance metric everywhere (0x4826f8): `max(|dx|,|dy|) + min(|dx|,|dy|)/2` (octile, cells).
- **Contact** (0x4a56c1): two armies meet when `|dx| ≤ 1 and |dy| ≤ 1` (8-neighbour
  adjacency). Relation (0x4a0868): let a = my attitude to his faction, b = his to mine; both ≥0
  → (a+b)/2; a<0 → a; else −1. Hostile adjacent: the AI attacks if its cached battle score
  is > 0 (AI vs AI battle 0x4a4c68; vs hero → encounter). Friendly adjacent: greets the hero
  (return 2) when its talk counter is > 0, then the counter is set to −500 and grows by
  (relation+1) per AI tick.
- **AIDistance0..2 are indexed by the behaviour style byte (59)**, not by the target model:
  feudal 100, rogue 50, peasant 25 cells (0x4a24f5, 0x4a2dcf). Armies farther than that are
  not (re)scored as targets.
- **AIGetPathDistance = 5**: the AI re-plans after 5 steps (0x4a3975), treats armies closer
  than 5 as obstacles (their cell and next cell, 0x4a3874), and flags a threat within 5.
- **Patrol area**: a square box `home ± radius` cells (Chebyshev), clamped to the map
  (0x4b4dde). Patrolling armies only take targets inside it (0x4a3110, 0x4a3308). Radius 0 =
  stationary: blocks the hero's and the AI's paths.

## 5. AI — M (structure H, some weights M)

- **Style** = DTm byte 59 (+0x16b7): AIDistance band, wages (only style 0 pays), respawn
  rule, village weight (×3 for non-feudal). Model byte 5 only picks the figure (+0x169d).
- **Target model** = byte 85 (+0x3824) indexes the 5-column lists
  (`0xc08058 + model·56`: AtackArmy, AtackCastle, Random, Talking, MinHeal, MaxHeal,
  MinGarrison, MaxGarrison, MinPurchase, MaxPurchase, GoldPurchase, MinVillage, MaxVillage,
  GoldVillage). The misspelt `MixHealingTarget` is not read (key is `MinHealingTarget`), so
  MinHeal = 0.
- **Goal choice is additive**, not multiplicative: every candidate target is seeded into one
  multi-source flood with its priority as the start value; the army walks toward the lowest
  `priority + path cost` (path cost in cost units, e.g. 10 per orthogonal grass cell).
  Lower = more attractive; seeds are capped at 32766 (0x482984, 0x4a2d88).
- **Army targets**: score from a full **simulated battle** (0x4a0710 runs the battle engine)
  (0x4a08f8): aggression (+0x16d0) moves the result by ±aggression % of each side's HP. If it
  wins: `1 + (own HP lost share) × ZeroDensity·30 × ownHP/enemyHP`; hostile winnable targets
  get `(AtackArmy[model] + score) × (relation + 4)`. If it loses: negative
  `−5 − √(enemy/own) × ZeroDensity × mySpeed/hisSpeed` (≥ −50) → a **repulsion field** around
  the danger (0x482e4c; 5× wider for stationary guards). Friends: talk seed
  `max(0, 800 − counter) + Talking[model]` when socialising is allowed.
  "Hunts only the player" zeroes other army targets; "ignored by AI" targets are skipped.
- **Healing** (0x4a2e6e): seed `MinHeal + (MaxHeal − MinHeal) × (1 − missingHP/maxHP)`,
  scaled up by cost/spare-gold when it cannot afford it; resurrection needs a town or church
  (seeded ×3 there). AI healing occupies the army for `HealingTime` minutes (0x4a65b2).
- **Villages** (0x4a0c05): `MinVillage + (MaxVillage − MinVillage) × (1 − stock/(GoldVillage +
  spare gold))` (never below Min); ×3 for rogue/peasant. Purchase and garrison use their
  Min/Max/Gold lists the same way (0x4a0fea, 0x4a14a4); castle attack uses AtackCastle×50 and a
  simulated fight against the garrison (0x4a11b0) — details L.
- **Random wandering**: 4 random points seeded with Random[model] when random targets are
  allowed or the army has idled > 10 plans (0x4a3742).
- **Spare gold** (0x4a0530): gold minus `NeedUpkeepDay` days of daily cost plus
  `NeedUpkeepDay − 1` days of income (feudal); rogues keep one day.
- **Respawn** (0x4a28d0): an inactive army with a home building, a respawn delay (byte 70 ×
  1440 min) and elapsed delay comes back at the **centre of its home building's footprint**,
  all units at full HP, gold += days × daily income. Rogue/peasant: also take ownership of the
  home if it is a village, shipyard, altar or dungeon entrance. Feudal: if it no longer owns
  its home, it uses a town, else castle, else fort it owns; owning none cancels respawn.
- **Economy at noon** (0x4a41d8, same routine for the hero = slot 0): income = extra daily
  income (**DTm byte 80 × 10**; word 17 is the army's starting *gold*, not income) + the gold
  stock of owned castles/forts (×`[0x68e784]`% for the hero) + stock of villages linked to an
  owned castle; stocks reset. Style 0 pays wages (Community code at 0xc250e4); if gold < 0 the
  cheapest units are left unpaid; a unit unpaid for > `MaxTimeNotUpkeep` minutes leaves.
  Other styles pay nothing.
- Lords retreating after defeat, rogues retaking forts outside respawn: **not found** (L/unknown).

## 6. Time costs and day ticks — H

- Wait: 1 h = 2 ticks, 4 h = 8 ticks of 30 game minutes, each tick one WalkDelay (0x4b9448,
  0x4ae351). Community F4 = endless ticks until F5 (0xc277d2).
- World spell cast: `TimeCast × 2` ticks = **TimeCast hours**, divided by 2 for the archmage
  (`[0x68e4f0]`); a Community flag (0xc25cf1) makes it ×0.8 for others (0xc27448).
- **Player healing and resurrection take no game time**: `HealingTime` is read only by the AI
  (0x4a65b2, 0x4a6750).
- **00:00** (0x4a1998, next midnight `0xc081a8`): villages' gold stock
  `+= round(income × √(1 − stock/max))`, capped (mana the same with its own fields) — refill
  slows as it fills; barracks: each type below max gains 1 with chance `1 / (MaxDayCountForNewUnit
  div max)` (certain when the quotient is 0); garrisons heal `GarrisonAutoHeal`% of max HP;
  armies with a unit of kind 4 (probably the medic) heal 10% of max HP per unit; building
  scores recomputed. Markets re-roll per building timer (0x4be178).
- **12:00** (next noon = `(day+1)·1440 + 720`, 0x4b4388, 0x4a41f9): income, wages, desertion
  for every army including the hero.

## 7. Hero start, entry, footprints — H

- Hero is placed **exactly on the preset x/y** (preset bytes 37/39, 0x4b585f); no relocation.
- The preset's start building (byte 16) and every building flagged for the class (byte
  353+class) are **given to the player** (owner = hero, faction/attitudes copied, 0x4b442a,
  0x4b5218) — they do not move him.
- Footprint (0x4b2f57): the stored x/y is the **bottom-right** cell; cells
  `x−sx+1..x × y−sy+1..y`, plus **one extra row above** when `sx > sy`. The anchor cell holds
  the building id; all footprint cells point to it.
- **No entry cell**: any footprint cell enters the building; footprints cost road speed. AI
  armies stand at the footprint centre `(x0 + sx/2, y0 + sy/2)`.

---

## Razdor now → original

Status after the world/movement/AI pass (branch `worktree-agent-a61eafdd222cb793e`): **done**
unless noted. "Before" is what Razdor did until then.

| Topic | Before | Original | Status |
|---|---|---|---|
| Grid | 8-neighbour squares; diagonal = √(32²+22²)/32 | 8-neighbour squares; diagonal ×1.5, vertical = horizontal | done (`Grid::weight` 2/3, `map::step_minutes`) |
| Road / grass / marsh | 30 / 60 / 120 min | 15 / 25 / 40 min (value×5) | done (`map::surface_value`) |
| Clay, stony, scorched | 75 | 25, 20, 25 | done |
| Sand / snow | 90 / 120 | 25 / 30 | done |
| Shallows | walked, 120 | **water**: ship only (10 min), not on foot | done (`is_water`, SHIP map) |
| Lava | walked, 120 | blocked | done |
| Deep sea | ship water | blocked for ships too | done |
| Hills / trees | +50% | +2 (+3 class 4) / +4 (+6 dead trees) cost units, additive | done (`object_effect`; hills set a base, plants and massifs add) |
| Massif cover | disc radius (f−1)/2, bottom on cell | square side f, object = bottom-right | done (`object_cells`); the "roads stay open under objects" guess is reverted |
| Step time | cost of entered cell | cost of the cell left (planner: entered) | done (`Game::step_time`, `TileMap::search`) |
| Ranger | ×1/1.2 | speed 4 vs 5 (×0.8) | done (`KNIGHT_SPEED`, `RANGER_SPEED`) |
| Speed correction | ±10%/pt | `max(1, 5−c)/5`; archmage-led AI −1 | done (`Army::speed_for`, also for the event that changes it) |
| Buildings | footprint blocks except an entry cell; gates | footprint walkable (road); any cell enters; only hostile/neutral castles, forts and unowned ruins block the hero's route | done (`place_buildings`, `Location::bars_hero`, the hero's mask in `Game::plan`); entry cells and gates removed; stationary guards and bridges at sea closed too |
| Hero start | building entry / nearest flagged building | preset x/y; flagged buildings become his | done (`World::start_buildings`, `give_to_player`); a preset on water starts him aboard *(guess)* |
| Sight | 7.5 cell widths on screen | 9/8/10 cells (knight/archmage/ranger), circle in cells | done (`fog::sight_radius`, edge +0.6 M) |
| Lantern unit | cell widths, guess | cells (×2 half-cells) | done |
| Unexplored impassable | yes | yes (hero only) | done (unchanged) |
| Chase / view | 6 × AIDistance by model | AIDistance by style: 100/50/25 cells scoring range; contact = adjacent | done (`ai::target_range`, octile distance, patrol box); the demo's gangs keep their 6-cell chase |
| Goal score | priority × (10 + distance) | priority + path cost (flood), battle simulated | done (`ai::choose`, `flood`, `simulate`, `battle_seed`); the repulsion field around a losing target is not modelled (it is simply no target) |
| Wait 1h/4h | as is | 2 / 8 ticks of 30 min | done (`WAIT_TICK_MINUTES`; the UI plays them at 150 ms each, `begin_wait`); casting uses the same ticks |
| Heal / resurrect time | HealingTime per unit | none for the player | done (`town.rs`) |
| Village refill | +income/day to max, at 00:00 | +income·√(1−stock/max) at 00:00 | done (`Location::refill`) |
| Barracks | deterministic progress | random per night, p = 1/(10 div max) | done (`Recruit::regrow`) |
| Garrison heal | at noon | at 00:00 | done (`Game::midnight`, `ai_midnight`) |
| Army word 17 | extra income | starting gold; income = byte 80 × 10 | done (`AiProfile::extra_income`) |
| Ship after landing | waits, re-boardable | route map returns to land-only on landing (M) | the ship waits at the last water cell and can be re-boarded (confirmed by the player's knowledge of the original; the M reading of the route map does not mean the ship is removed) |

Also done from §2/§5: real-time pacing (`STEP_SECONDS` = 150 ms per step or wait tick), the
AI clock (banked minutes, cap 200), respawn at the home building's centre (feudal fallbacks,
rogues take their home), AI armies standing at the footprint centre.
