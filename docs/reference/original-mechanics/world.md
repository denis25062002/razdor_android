# Discord Times: world map, movement, time and sight (from the user's Community exe)

Rules in plain words. Addresses are VAs in the user's `DiscordTimes.exe` (image base 0x400000);
they point to the code that does it. No code is reproduced. Confidence tags: **code** (read in
the code), **data** (consistent with data, help or play, not traced), **unknown**.

World AI (goal choice, target scores, healing, respawn, economy of AI armies) lives in
`ai.md`; §5 here keeps only what the world needs from it. Economy of the noon and the night
(income, wages, refills) lives in `economy.md`; §6 lists only when those moments happen.

Runtime layout used below (for orientation only):
- Army slots: slot 0 = hero, slot k = DTm army k (1-based), slot N+1 = the hero's parked ship
  (N = number of armies); 0x3827-byte records at `0x75a940 + k*0x3827`. Field offsets below
  are relative to that record.
- Occupant codes in the cells: 1 = hero, k+1 = army k, N+2 = the parked ship. Two tables
  per code: the logical cell (`0x75a140 + 4*code`) and the drawn cell (`0x75a540 + 4*code`).
- Cell records (18 B) at `[0x68ecb0]`, index `y*(W+8)+x`, coordinates 0-based: +0 massif object
  (class<<8|sprite, classes 1–8), +2 plant object (classes 9–12), +4 building id at its anchor,
  +6 point id (lantern / event point), +8 logical occupant code, +9 drawn occupant code,
  +0xC anchor index of the building whose footprint covers the cell.
- Terrain bytes at `[0x68ecac]`, (W+2)×(H+2) with a copied border: cell (x,y) is byte
  `(y+1)*(W+2) + x+1`.
- Three 16-bit cost maps of W×H built at map load (0x4b31dd–0x4b391d): LAND `[0x68e790]`,
  MIXED `[0x68e794]`, SHIP `[0x68e798]`. 0 = impassable.
- Time `[0x68dcb8]` is **centi-minutes** since scenario start; absolute minutes =
  `[0x68dcb8] div 100 + [0x68dcbc]` (0x49d631, 0x4abfd6).

---

## 1. Grid, cost maps and the hero's planner — code

**Grid: square, 8 neighbours.** Neighbour table dx `0x4ecf8c` = (−1,0,1,1,1,0,−1,−1),
dy `0x4ecfb0` = (−1,−1,−1,0,1,1,1,0): direction 0 = north-west, then clockwise (1 N, 2 NE,
3 E, 4 SE, 5 S, 6 SW, 7 W); even directions are diagonal, odd ones orthogonal; 8 = no
direction. Step weight table `0x4ecfd4` = diagonal 3, orthogonal 2. Screen cell 32×22 px
(0x4b2c79, 0x4aea08). No hex anywhere. No corner-cutting rule: a diagonal step between two
blocked orthogonal cells is allowed (0x482a58).

### 1.1 Building the cost maps (0x4b3285–0x4b391d)

Terrain table `0x4ed128`, object table `0x4ed164` (one entry per class). Steps in this order:

1. All three maps start at an "untouched" marker.
2. The cell grid is scanned row by row (y, then x, over the cell stride). A massif of class 1–4
   (hills) **sets** LAND to its class value, MIXED to 5× that, SHIP to 0, over its square (see
   below). Where hills overlap, the one later in that scan wins.
3. Every cell of the map: if LAND is not 0, the untouched marker counts as 0, the terrain value
   is added, a result below 0 becomes 0 (blocked). Terrain code 0–2 is **water**: LAND = 0,
   MIXED = SHIP = the result. Codes ≥ 3: LAND = result, MIXED = 5× result, SHIP = 0.
4. Plants (class 9–12) on their own cell: if LAND is not 0 their value is added (below 0 → 0).
   Then the cell is rewritten as land whatever it was: LAND = value, MIXED = 5×, SHIP = 0.
   **A plant standing on water therefore blocks ships** (its LAND was 0, so everything becomes
   0).
5. Massifs of class 5–8 over their square, same rule as plants with −32: every cell becomes 0
   on all three maps. **Mountains and rocks over water block ships too.**
6. Every building footprint (§7.1) is then overwritten: LAND = 3 (road), MIXED = 6, SHIP = 3.
   Buildings and bridges are walkable and sailable at road speed.

Terrain values (cost units; minutes per orthogonal step on foot at speed 5 = value × 5):

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

Object class values: 1,2,3 = 2 (hills); 4 = 3; 5,6,7 = blocked (mountains); 8 = blocked
(rocks); 9 = +4 (trees); 10 = +6 (dead trees); 11 = blocked (thicket); 12 = +4. Examples:
hill on grass = 7 (35 min), tree on grass = 9 (45 min), hill on coastal water = 3 by ship.

**Massif footprint** (classes 1–8): a square of side `f = sprite div 10` cells whose
**bottom-right** cell is the object's cell (rows y−f+1..y, columns x−f+1..x). It is clipped
only on the east and south edges. Edge case, code: a square reaching past the **west** edge
wraps into the last cells of the row above, and one reaching past the **north** edge writes
outside the map (a reimplementation should simply clip). Plants cover only their own cell.

### 1.2 The planner (0x482828–0x482fe8)

One planner object per mover; the hero's is at 0x68bc10 and works on LAND, or on MIXED while
he is at sea or has just bought a ship (§8).

- **Seeds** (0x482984): a seed is a target cell with a start value. A cell whose cost is 0 on
  the planner's map is refused. The value is capped at 32766 and stored plus 1. The hero's
  click makes one seed, the clicked cell, with value 0 (stored as 1).
- **Effective cost** (0x482a58): before the flood every cell's cost is multiplied by a mask
  value (0 closes it, 1 keeps it; the AI adds larger values, `ai.md`). The product is 16-bit.
  Quirk: cell (0,0) is skipped by that loop and keeps the bare mask value as its cost.
- **Flood** (0x482a58), from the seeds outward, towards a stop cell (the hero's cell):
  1. Every distance starts at "unreached" (65535); seeds get their stored value; the frontier
     is the seed list in order.
  2. A threshold starts at 1. Each round walks the frontier in order: an entry whose distance
     is above the threshold is carried over unchanged; any other is expanded: for directions 7
     down to 0, the neighbour (inside the map) gets `entry distance + effective cost of the
     neighbour × weight(direction)` if that is strictly smaller than its distance (a closed
     neighbour never improves). Improved neighbours are appended to the new frontier.
  3. **As soon as the stop cell's distance is improved for the first time, the whole flood
     stops.** Otherwise the threshold becomes the smallest distance in the new frontier and
     the next round starts; the flood ends when the frontier is empty.
  4. Distances above 65534 are never stored (a natural range cap).
  Because the flood starts at the target, the cost charged for each step is the cost of the
  cell **being left** in walking order (the start cell is charged, the target cell is not).
  Because it stops at the first value reaching the hero, the route can be slightly worse than
  the cheapest one (by at most about one step near the hero).
- **Reading the route** (0x482fe8), from the hero's cell: repeatedly step to the neighbour with
  the **smallest distance** that is non-zero and strictly smaller than the current one;
  directions are tried 0..7 and the first strict minimum wins ties. It stops when no
  neighbour is smaller. Each step stores (x, y) and the direction from the previous cell.
  The route cost kept with it adds, per step, the base cost of the cell left (×1.5 rounded
  down on diagonals); it feeds the "time left" in the status bar (§2.4).
- If the hero's own cell stayed unreached, there is no route.

### 1.3 The hero's mask and the click (frame 0x4cc148, hit test 0x4cbf20)

**Valid target** under the cursor (0x4cbf20): inside the map view (above the bottom panel at
y 682), not over the minimap, on an **explored** cell, and either with a non-zero cost on the
hero's current map or holding the parked ship. A bridge is not a valid target while at sea.
Clicking an unexplored cell does nothing.

**First click on a cell: plan and show** (0x4cc426–0x4cc99a):
1. The clicked building and the building the hero stands in are remembered.
2. Seeds cleared, mask all open. A click on the parked ship gives its cell cost 1 for the
   duration of this plan (water costs 0 on LAND). Seed = the clicked cell.
3. If the hero stands in a building, its footprint is re-set to road cost on his map (6 at
   sea).
4. The mask closes:
   - cells of armies that **patrol with radius 0** (patrol flag +0x16bb set, radius +0x16bc
     = 0; stationary guards) and are on the map, except the clicked army;
   - cells of armies with a **meeting event waiting** (+0x3826) standing at distance exactly 1
     of the hero (§4.1 distance, i.e. any of the 8 neighbours), except the clicked army;
   - footprints of castles and forts whose attitude to the player (+0x152) is ≤ 0, and of
     ruins whose owner is not the player — except the clicked building and the one he stands
     in;
   - when the hero is at sea and either stands in a bridge or clicked a land cell: every
     bridge footprint;
   - every **unexplored** cell (the explored image is stamped over the mask, 0x4cc857).
   The hero's own cell is reopened before the fog step.
   Other buildings (towns, villages, churches…) are **not** closed: the route may cross them.
   Moving armies are not closed either (the AI keeps them off his cells instead, §5).
5. Flood from the seed to the hero; if he is reached, the route is read and drawn, the
   clicked cell becomes the **planned target**, and the clicked army becomes the **chased
   army** when the click was on an army that is not inside a building other than a bridge.
The bridge closing skips the clicked building and the one he stands in, like the castle rule.
**Second click on the planned target: walk** (0x4cc99f). Any other click plans again. A click
on the building the hero already stands in opens its window. Quirk, code: the walk only
starts when the target's x or y is above 0, so a route to cell (0,0) is shown but never
walked.

**Chasing** (0x4ade3c, 0x4aedd1): with the "pursue the chosen army" option (OptValue7,
`[0x65d4db]`), when the chased army finishes a step in a frame where the hero has also just
finished one, the hero's route is planned again to the army's new cell from the cell he
has reached, with the same mask (the hero's cell is not reopened this time; the "clicked
land" test for bridges reads the cell under the mouse cursor at that moment, and the
clicked/standing buildings are the ones from the original click). If the army's cell is
closed or unreachable the chase ends and **the hero stops** on the cell he has reached.

## 2. Walking, speed and real-time pacing — code

### 2.1 Step time

- Hero step time (centi-minutes) = `cost of the cell he leaves × speed × 100`, ×1.5 on a
  diagonal step (0x497c68, 0x4ae954). The cost is read on LAND, or on MIXED at sea. All
  products are exact integers (×150 for a diagonal).
- Speed by class (0x4b4300): **knight 5, archmage 5, ranger 4** → the ranger's steps take 80%
  of the time. Minutes per orthogonal grass step: 25 (ranger 20).
- The class values (sight, speed, cast divisor) are set when the map is started (0x4b4300).
  The save writes and reads class, sight and speed (0x4b66d8 / 0x4b771c); the cast divisor is
  not saved but recomputed from the loaded class (archmage 2, others 1; 0x4b78b2). No event
  result changes the class, so a changed hero unit does not change these values.
- Community, code: an event result can set any army's speed byte, the hero's included
  (0xc279e6); the hero's step time uses that per-army value, not the class value.
- AI army speed (loader 0x4b4824): `max(1, 5 − speed correction)` (DTm byte 13), then −1 if
  the leader unit (byte 26) is GlobalIndex 2 (the archmage unit), clamped ≥ 1. One point of
  correction is 20% of the base time.

### 2.2 One step, frame by frame (walk timer 0x4ae6dc)

Every hero step plays over **WalkDelay** ms of real time: `150 + (100 − WalkSpeed) × 250 div
100` (0x4b8c4f; WalkSpeed 100 → 150 ms, 0 → 400 ms). The real time of a step does not depend
on its cost.

1. **Start of the step** (first frame after the previous step ended): the next cell is
   checked for contact (§4.2). Without contact, the hero's *drawn* cell moves to the next
   cell, the route arrow under him is cleared, and a new **AI tick** starts whose amount is
   this step's time (§5).
2. **During the step**: game time = time at the step start + `Round(elapsed / WalkDelay ×
   step time)`, so the clock runs smoothly; the camera and the hero's figure slide linearly
   between the cells. The contact check is repeated only while the step has not started
   visually. Fog is revealed around the cell being left, shifted half a cell towards the next
   cell during the first half of the step (§3).
3. **End of the step** (elapsed ≥ WalkDelay): the whole step time is added to the clock, the
   step counter advances, the hero's *logical* cell becomes the new cell (0x497c68), his step
   time is recomputed from the new cell (×1.5 if the following step is diagonal), the camera
   centres on him (cell − (14, 16)), and the cell's point id is noted for the event scan.
4. After each frame: the AI armies advance by the game time that passed (0x4ade3c). After an
   end of step: the events are scanned (§6.4); an event that fires stops the walk (except the
   noon report). When the last step ends, the hero stops (0x4ae5d8) and, if he is in a
   building other than a bridge or an obelisk, its window opens (§7.2).
- The first step's diagonal factor is applied when the walk starts (0x4cc9c2).
- Hero walking animation: frame `(t div (WalkDelay/2)) & 3` + 3, where t is the real time
  since the **walk** started (not since the step), so the 4 frames cycle over two steps
  (0x4ae8f2, 0x4ad314). At sea the ship figure cycles 8 frames by real time (ms div 100).

### 2.3 Ships

No separate speed: at sea the hero uses the MIXED map, so coastal water costs 5 min and
shallows 10 min per orthogonal step (ranger 4/8); leaving a building costs 6 (30 min). Land
is priced 5× its cost for the planner only: he normally stops on the first land cell (§8).
AI ships use the SHIP map (0x4a1ff0). Details in §8.

### 2.4 Status bar

While a route exists (planned or being walked) the bar shows the time left: `(route cost ×
speed × 100 + route start time − now) div 100` minutes (route start = now if not set yet).
Below 60 the value is set to 0, which the formatter shows as the fixed under-an-hour text
(0x49d5dc). It uses the same formatter as the clock (§6.3), month field + 1 included.

## 3. Sight, fog and lanterns — code

- Sight radius per class (0x4b4311): **knight 18, archmage 16, ranger 20 half-cells**
  (= 9 / 8 / 10 cells).
- **Two images**: a soft **fog map** at twice the cell resolution (`[0x4ed46c]`, values from
  0 = clear to the darkest), drawn over the map each frame from a 68×68 window around the
  camera; and a hard **explored image**, one flag per cell (`[0x68e79c]`), which decides
  passability for the hero's planner, the cursor and the minimap. Neither is ever darkened
  again: **explored ground stays visible for good** (all writes keep the brighter value).
- **Stamps** (built once, 0x4cf734): for every radius r = 0..48 half-cells, a 98×98 half-cell
  mask centred between its two middle pixels. For a half-cell at distance d (half-cell units,
  single precision), brightness = `Round(15 − 8·(d − r))` clamped to 0..15 (round half to
  even). So it is full up to r and fades out over the next 1.9 half-cells.
  The matching explored mask is 49×49 cells: a cell is explored when the brightness of its
  four half-cells adds up to more than 20 (the code tests the sum of the darkness values,
  scaled, against 10).
- **Reveal** (0x49c8f0): the fog mask is merged (keeping the brighter value) centred on the
  cell, and the explored mask is stamped over the explored image. Resulting explored cells:
  radius 8 cells → 241 cells, 9 → 293, 10 → 349, 24 → 1901. A Euclidean disc of
  `r + 0.62` cells reproduces the three sight radii exactly; no single edge reproduces every
  lantern radius (radius 20 fits none), so a faithful port should use the half-cell rule.
- When: every frame of a step, around the cell being left (§2.2); at a step boundary reached
  before the step was drawn (0x4ae829); and at every stop (0x4ae5d8). At the start of the map
  the hero's reveal **grows** from 0 to his sight (timer 0x4af83c: radius = elapsed ms × 2 div
  50 half-cells, camera centred on him).
- Lanterns: those active at start reveal `radius × 2` half-cells at once (0x4b5a37), so the
  DTm radius (≤ 24) is in **cells**. Event lanterns: the camera scrolls to each one and the
  reveal grows like the hero's start reveal (0x4ab73c → 0x4af96c → 0x4af83c). A lantern with
  radius 0 is **skipped** in both places and reveals nothing (code: both loops test radius >
  0). "Show army" results scroll to the army and grow a reveal of 6 half-cells (3 cells)
  there (0x4ab6c3).
- Unexplored cells are **impassable to the hero's planner** and not clickable. The AI's
  planners do not use the explored image (the AI ignores the fog). Armies are not hidden
  logically by the fog; only the overlay covers them (data).

## 4. Distance, contact and meeting — code

### 4.1 Distance

Everywhere (0x4826f8): `max(|dx|,|dy|) + min(|dx|,|dy|) div 2` (cells). Distance 1 = any of
the 8 neighbours.

### 4.2 The hero's step (0x4ad94c)

Checked for the cell he is about to enter, before he moves (§2.2):

1. **An AI army stands there** (occupant code of an army, not the ship): it is engaged,
   **friendly or not**, unless the cell belongs to a town, tavern, church, smithy, shipyard,
   altar or dungeon (types 1, 5, 7–11), or the army's attitude to the player is > 0 and the
   cell is a building other than a bridge. On open ground every army is engaged.
2. Else, **a building cell** of a village, castle, fort, ruins or bridge (types 2–4, 12–14):
   the last on-map army whose home building it is guards it; the guard is engaged when its
   attitude to the player is ≤ 0, or always on a bridge.
3. No guard: a castle, fort or ruins whose attitude (+0x152) is ≤ 0 (ruins always) with a
   garrison record: an empty garrison means the building is **captured** (owner = player,
   the hero's faction and attitudes copied); otherwise the garrison is engaged. An unguarded
   **village is captured** by stepping on any of its cells — also when the route only passes
   through it.
4. No engagement and the hero is at sea: if the cell is land, or a building other than a
   bridge, the route is cut so that he walks onto that cell and stops; he leaves the sea and
   the ship is parked on the water cell he leaves (§8). Bug, code: the "is it land" test reads
   the terrain row as `cell index div (H+2)` instead of `div (W+8)`, i.e. a cell a few rows
   further south; the building test is right. When the misread cell is water he is not
   stopped: he steps onto the land, the end of the step takes him off the sea from the correct
   terrain (0x497c68) without parking the ship (it is lost), and he walks on along the route on
   foot.
5. Engagement: the walk stops with the hero on the cell he was leaving, the opponent becomes
   the "met army" for event conditions, and the events are scanned; if none fires, the battle
   screen opens. If an event fired instead, that army's AI memory of the hero is reset.

### 4.3 AI armies reaching the hero (0x4a548c, called after each AI step)

Two armies are **in contact** when `|dx| ≤ 1 and |dy| ≤ 1` (0x4a56c1). Relation (0x4a0868):
let a = my attitude to his faction, b = his to mine; both ≥ 0 → (a+b) div 2; a < 0 → a;
otherwise −1. A hostile army in contact attacks the hero when its cached battle score
against him is > 0; a target standing in a building is attacked only when the building is a
bridge or the target's own (otherwise both armies' scores against each other are tripled, cap
10000; `ai.md`). A friendly army in contact greets the hero (meeting) only right
after the hero finished a step and only if its talk counter towards him is > 0; then the
counter is set to −500 on both sides. Talk counters grow by `relation + 1` per AI step while
the relation is ≥ 0, plus 1 per step for every other army at distance > 0 (0x4a399c). A
meeting is handled before an attack in the same frame (0x4ade3c).

Both the attack and the greeting are acted on only in a frame where the hero has **just
finished a step** (the hero's "arrived" flag, set only by the walk timer, 0x4ae977; tested in
0x4ade3c). So while he waits or casts, AI armies move and bank time but never attack or
greet him; an AI step that ends next to him in another frame is ignored (it is tested again
after its next step). A greeting runs the event scan with that army as the met army; only
an event that fires stops the walk.

### 4.4 View, patrol and planning ranges

- **AIDistance0..2 are indexed by the behaviour style byte (59)** (feudal, rogue, peasant;
  values from the rules file; 0x4a24f5, 0x4a2dcf). Farther armies are not (re)scored as
  targets.
- **AIGetPathDistance** (rules file, `[0x4ed508]`): an AI army plans again when its step
  countdown runs out or when another active army (the hero included) is at distance 1 up to
  that value after one of its steps (0x4a3975); see `ai.md`.
- **Patrol area**: a square box `home ± radius` cells, clamped to the map (0x4b4dde), only for
  armies with the patrol flag. Radius 0 with the flag = stationary: it never steps and it
  closes its cell to the hero's planner.
- **"Move army to the hero"** event result (0x4980d8): of the hero's 8 neighbours (directions
  0..7), the one with the lowest score is taken, score = its cost (LAND, or SHIP when the hero
  is at sea; blocked = 100000) + 50000 if it is a building cell + 100000 if someone stands
  there; ties go to the lower direction. Only a free, passable cell (score < 100000) is used;
  the army's home and position both move there (the patrol box is not recomputed).

## 5. World AI — pointer (details in `ai.md`)

What the world needs:
- **The AI clock** (0x4a399c, run every frame with the game time that passed, only while the
  hero walks, waits or casts): each new hero step or wait tick adds that tick's time to every
  moving army's bank (capped at 20000 centi-minutes = 200 min) and opens a window of that
  length. An army ready to move takes one step when its bank covers `cost of its current
  cell × speed × 100 × weight / 2` — **the cell it leaves**, like the hero. The step's play
  time is its cost scaled by `tick / bank` when the bank also covers the following step,
  otherwise the rest of the window, never longer than what is left of the window; when the
  play time runs out the army arrives and may take the next step. Stationary guards (patrol
  flag with radius 0) are skipped entirely.
- **The hero's cells are never entered**: if an AI step would go onto the hero's cell or the
  cell he is stepping to, the army spends the time but stays put (and is then in contact).
- AI armies stand at the centre of their home building's footprint `(x0 + sx div 2,
  y0 + sy div 2)` when they respawn.
- Boarding or leaving the sea (§8) empties the banks and route countdowns of the AI armies on
  the other medium (0x496d28).
- An AI army runs its own noon (income, wages) only when it finishes a step after its noon
  time (0x4a548c): armies that never step skip it.

## 6. Waiting, casting and the daily ticks — code

### 6.1 Waiting

- Wait buttons (0x4b9448): 1 h = 2 ticks, 4 h = 8 ticks; a third button scrolls to the hero.
  One tick = 30 game minutes played over one WalkDelay of real time (0x4ae280); between ticks
  the clock runs smoothly (`Round(elapsed / WalkDelay × 3000)` centi-minutes). Starting a wait
  drops the route. Each tick starts a new AI tick of 30 minutes.
- After each tick the events are scanned (the point under the hero counts); an event that
  fires ends the wait. Community F4 = endless ticks until F5 (0xc277d2, 0xc27802); in that mode
  an event's dialog opens without ending the wait (0xc2782b; whether the modal dialog pauses
  the ticks is **unknown**).
- Event delays use the same ticks: hours × 2.

### 6.2 Casting a world spell

`TimeCast × 2` ticks, integer-divided by the cast divisor (archmage 2, others 1; `[0x68e4f0]`),
so TimeCast hours (half for the archmage). A Community flag (0xc25cf1, the Caster bonus) makes
it `floor(× 0.8)` for the others (0xc27448). The cast is abandoned if its target army leaves the
map or is destroyed during the ticks (0x4ae536).

**Player healing and resurrection take no game time**: `HealingTime` is read only by the AI
(0x4a65b2, 0x4a6750).

### 6.3 Calendar

- Start: the clock reads the DTm start minute **plus 1** (0x4b42d8), game time 0.
- Display (0x49cbf0): year = minutes div 518400 (360 days), month = (minutes div 43200) mod
  12, shown + 1; day = (minutes div 1440) mod 30, **shown 0-based**; hour = (minutes div 60)
  mod 24. Leading zero fields are left out; under an hour a fixed text is shown.

### 6.4 00:00 and 12:00

- **12:00 for the hero** (0x4abfbc): checked in every event scan (after each step, tick or
  dialog) when no event fired and no spell is being cast, against the next noon `[0x75c04c]`.
  At map start the next noon is `(start div 1440 + 1) × 1440 + 720`: **always the noon of the
  day after the start day**, even for a morning start (0x4b4388). After each noon it is the
  next day's noon (0x4a41d8). With no wages and no castle income the noon is paid at once and
  the game autosaves; otherwise the noon report opens (and pays when shown). Ranger healing
  and the pay itself: `economy.md`.
- **12:00 for an AI army**: first noon `(now div 1440) × 1440 + 720`, a day later if already
  past (0x4a1ff0), then processed after its steps (§5).
- **00:00** (0x4a1998, next midnight `[0xc081a8]` = `(now div 1440 + 1) × 1440`, set at map
  start and after each midnight): checked every frame of AI advance, after the armies moved.
  Markets re-roll, barracks may grow (a type below its max gains 1 with chance
  `1 / (MaxDayCountForNewUnit div max)`, certain when that is 0; one roll of the game RNG per
  slot), villages refill gold and mana, garrisons heal `GarrisonAutoHeal`% of max HP, armies
  with a unit of kind 4 heal 10% per wounded unit (the hero's army included, slot 0), AI
  building scores are recomputed. Formulas: `economy.md`.
- Time only runs while the hero walks, waits or casts; standing still freezes the clock.

## 7. Hero start, footprints and entering buildings — code

### 7.1 Start and footprints

- The hero is placed **exactly on the preset x/y** (preset bytes 37/39, 0x4b585f); no
  relocation. The camera centres on him and his first reveal grows (§3).
- The preset's start building (byte 16) and every building flagged for the class (byte
  353+class) are **given to the player** (owner = hero, faction/attitudes copied, 0x4b442a,
  0x4b5218) — they do not move him.
- Footprint (0x4b2f57): the stored x/y is the **bottom-right** cell; cells `x−sx+1..x ×
  y−sy+1..y`, plus **one extra row above** when `sx > sy`. The anchor cell holds the building
  id; every footprint cell points to the anchor. After loading, the building record holds the
  top-left corner.

### 7.2 Entering

- **No entry cell**: any footprint cell can be the target, and every footprint costs road.
- A building is **entered** (`[0x68dc74]` set, 0x497c68) when the hero arrives on one of its
  cells coming from another building cell, or when his walk **ends** on it (the stop repeats
  the arrival on the same cell). Passing over a single cell of a building does not enter it;
  crossing two or more consecutive footprint cells does (its local events can then fire,
  §6.4 scan) without opening its window.
- Leaving onto a cell outside any building (or onto a bridge) clears it; a bridge counts as
  "the building the hero is on" for events but never as entered.
- When the walk ends inside a building other than a bridge or an obelisk (types 13–15), its
  window opens (0x4aed85 → 0x4bbc84).
- Event scan order (0x4abfbc): global events, then the event point under the hero (a single
  cell), then the building he is in (only local events there, except in villages and
  shipyards where all its events count).

## 8. Ships and water travel — code

- **Buying** (shipyard window, 0x4c60ac): costs `[Costs] ShipCost`. It removes any ship the
  hero already owns (one ship only; this also clears `EnterShipyard` and the ship's home
  shipyard, 0x49704c), and switches his planner to the MIXED map. No ship object appears yet. From the shipyard's footprint he can now route
  onto the adjacent water.
- **Going to sea** (0x497c68, 0x497abc, 0x496d28): the hero is at sea as soon as he steps out
  of a building or onto any non-building cell whose terrain is water, or moves inside the
  shipyard with the MIXED map. Being at sea sets the event flag `Sea` and clears
  `EnterShipyard`.
- **At sea**: routes are priced on MIXED (water cheap, land 5×, buildings 6), so the planner
  keeps to the water and lands at the last moment. Bridges are closed to him when he clicks
  land or stands in a bridge (§1.3).
- **Landing** (0x4ad94c): stepping towards land or a non-bridge building ends the route on that
  cell; he leaves the sea and the ship (army slot N+1) is **parked** on the water cell he
  left, facing his direction. If he walks straight from a shipyard's footprint onto land
  after buying, no ship is parked and the purchase is lost.
- **Re-boarding**: the parked ship's cell is a valid click even though water costs 0 on LAND;
  walking onto it puts him at sea again. How the parked ship's own marker is cleared when he
  boards it is **unknown** (not found).
- **Shipyards**: landing into a shipyard from the water makes it the ship's home
  (`[0x671f78]`); entering that shipyard later sets the event flag `EnterShipyard`, leaving it
  clears the flag.
- **Ship armies of the scenario**: an army whose first unit type is the ship marker places or
  removes the hero's ship when an event activates or deactivates it (0x4969b8, 0x496900). AI
  armies loaded on water use the SHIP map and ship figures by DTm byte 72 (loader 0x4b4824,
  0x4a1ff0).

## 9. Minimap and camera — code

- **Image** (built at map load, 0x4b3a13–0x4b428a): one pixel per cell with a 2-pixel margin;
  terrain colour per code (Community colour table 0xc365a0); hills, then mountains and rocks
  over their squares, then plants on their cells, blended with each sprite's colour by its
  alpha; bridges as a light grey rectangle; then a contrast boost on all channels (×1300/1024,
  −33).
- **Size**: 200 px when the map is narrower than 100 cells, else 400 px (0x49df9c). Markers:
  every building except bridges and obelisks, every on-map army outside a building, and the
  hero, coloured by side (Community hooks, `interface` notes). The unexplored area is darkened
  by a blurred copy of the explored image (3×3, centre weight 4, neighbours 3; 0x49c700).
- **Click** on the minimap: the camera goes to `((mouse − minimap left) × W / size − 15) × 32`
  and `((mouse − minimap bottom + size) × W / size − 15) × 22` pixels (both axes scaled by the
  width), i.e. the clicked cell about 15 cells from the view's corner (0x4ccf5a).
- **Camera**: edge scrolling within 5 px of a screen edge and the arrow keys move it by
  `frame ms / k` px horizontally and 0.6875 × that vertically, `k = (1 − ScrollSpeed/100) ×
  1.5 + 0.5` (0x4b8c4f, 0x4cd016). It is clamped to `[32, (W+2)·32 − 1056] × [22, (H+2)·22 −
  704]` px (0x4c8864). While walking it follows the hero at (cell − (14, 16)) × cell size.

---

## Razdor now → original

Razdor's code read for this table: `src/rules/map.rs`, `fog.rs`, `game.rs`, `world.rs`,
`ships.rs`, `clock.rs`, `src/ui/world_view.rs`, `minimap.rs`.

| Topic | Razdor now | Original | Gap |
|---|---|---|---|
| Grid, weights | squares, 8 neighbours, weight 2/3, diagonal ×1.5 | same | none |
| Terrain and object values | `surface_value`, `object_effect` as §1.1 | same | none |
| Hill overlap | later in file order wins | later in row-major cell scan wins | small: order of overlapping hills |
| Plants / massifs on water | plant value added to the water cost; ships can sail | the cell is blocked for ships too | **fix** in `TileMap::from_codes` |
| Massif at west/north edge | clipped | wraps / writes out of the map | keep clipping (document) |
| Buildings on the maps | road on LAND and SHIP, 6 at sea | same | none |
| Planner algorithm | A* from the hero, pricing the cell **entered** | flood from the target, pricing the cell **left**, stops at the first value reaching the hero, route by steepest descent with direction-order ties | **fix** for identical routes (`TileMap::search`, `path_cost`) |
| Click into the dark | walks towards the nearest explored cell, re-plans as ground appears | not a valid target, nothing happens | Razdor extra (keep or drop) |
| First / second click | first click shows the route, second click walks (`world_view.rs`) | same | none |
| Mask: armies | every army's cell closed (player's request) | only stationary guards and meeting-waiting armies next to him | known deviation |
| Mask: buildings | castles/forts with attitude ≤ 0, ruins not his, **and any other hostile building** | castles/forts with attitude ≤ 0, ruins not his only | Razdor extra (`Location::bars_hero`) |
| Mask: bridges at sea | always closed at sea | only when clicking land or standing in a bridge | small |
| Hero step time | cost of the cell left × speed, ×1.5 diagonal | same | none |
| AI step time | cost of the cell **entered** (`step_army`) | cost of the cell **left** | **fix** |
| AI never enters the hero's cells | not modelled | waits in place, then contact | add |
| Stationary guards' clock | bank like everyone | skipped | small |
| Pacing | 150 ms per step / wait tick, game time added per step | same; game time also interpolated inside the step | none for rules |
| Contact on the hero's step | adjacency after the step (`contact`, distance ≤ 1) | the cell he steps onto holds an army (any army on open ground); AI adjacency after AI steps | different trigger; outcome close |
| Village crossed on the way | not captured | an unguarded village is captured when crossed | **fix** |
| Building entered when crossed | only the clicked/last one | entered when 2+ footprint cells are crossed (events may fire), window only at the end | small |
| Friendly meeting | once while nearby, again when clicked (`met`, `MEET_AGAIN_DISTANCE`) | talk counters, −500 after each meeting, grow per AI step | approximation |
| Sight radii | 9/8/10 cells | same | none |
| Explored edge | Euclid `r + 0.6` | half-cell rule; `r + 0.62` fits the sight radii; archmage gets 8 more cells | **fix** `fog::EDGE` (or port the rule) |
| Start reveal | instant | grows over ~0.4 s, camera on the hero | cosmetic |
| Lantern radius unit | cells | cells | none |
| No re-fogging | yes | yes | none |
| Clock start | DTm start minute | start minute + 1 | 1 minute |
| First noon | the first 12:00 crossed (start day if before noon) | always the next day's noon | **fix** for parity |
| Day shown | 0-based (`Clock::day`) | 0-based | none |
| Wait 1 h / 4 h | 2 / 8 ticks of 30 min | same | none |
| F4 endless wait | not present (`hotkeys.rs`: F5 is quick save) | Community: F4 waits until F5 | Community extra, optional |
| Casting time | wait ticks | same | none |
| Heal / resurrect time | none for the player | none | none |
| Ship purchase | rented ship waits at a mooring (guess) | no ship object; planner switches to MIXED in the shipyard | Razdor guess |
| Landing | lands on any walkable cell next to water; ship waits at the last water cell | same, but the landing test reads a cell further south (bug); building cells also land | bug not reproduced |
| Ship lost by walking out on land | no | yes | small |
| Move army to hero | first passable non-building neighbour, army activated *(guess)*, home kept (`script.rs`) | lowest-score free neighbour (building cells only as a fallback), home moves too, not activated | small |
| Event lantern radius 0 | 5 cells (`LANTERN_RADIUS`) | nothing revealed (radius 0 skipped) | small |
| AI attack while waiting | not checked in this pass | never: AI attacks and greetings only in the frame the hero finishes a step | to check |
| Chase target unreachable | not checked in this pass | chase ends and the hero stops | to check |
| Show army reveal | 3 cells | 3 cells (6 half-cells), growing | none |
| Minimap size | 400×400 frame | 200 px under 100 cells wide, else 400 | small |
| Minimap click | centres the view | clicked cell ~15 cells from the view corner | small |
| Hero class change by event | speed and sight follow the new unit (`hero_class` from the hero's unit) | sight, speed and cast divisor keep the starting class (Community: an event may set the hero's speed directly) | **fix** for parity |

## Unknowns

- What the battle screen does when the hero steps onto a **friendly** army on open ground
  (the code starts an encounter unless an event fires; `[0x669dee]` = 1 marks a hero-made
  contact).
- How the parked ship's marker is removed when the hero boards it again.
- Whether a modal event dialog pauses the Community endless wait (F4).
- The meaning of the AI bounds test that takes an army off the map when it stands beyond two
  record fields (+0x376a/+0x376e, 0x4a399c).
- The exact drawing order of the fog overlay against armies and buildings (data says the fog
  covers them).
- How large the planner's early stop error gets in practice (bounded by about one step; not
  measured).
