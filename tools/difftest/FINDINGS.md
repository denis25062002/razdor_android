# Diff test findings

Differences between Razdor and the original Discord Times found by the diff test
(`run.py`). Each entry gives the replay, the step, the field and both values, the cause and
the spec reference. Each entry says whether a later commit fixed it.

Run: `tools/difftest/rk1-day1.jsonl` (РК1, knight; the first day, then the ruins' garrison
fought by explicit actions), `python -m tools.difftest.run --actions
tools/difftest/rk1-day1.jsonl --name rk1-day1`, original's music held off, draw trace on;
§5 from a second run with the Frida trace (`--name rk1-frida --trace random,ai,events,damage`).
"Step-local" is the run where each Razdor step starts from the original's generator state.

## 1. No draw for the idle patrollers when the hero stops

**Status: fixed in 1c1696f** (`Game::armies_snap`; world.md §2.2.1).

- Steps 2, 5, 7, 9, 12, 14, 16 (every stop of a walk or a wait). Step 5 (`wait 1`),
  step-local: `rng` Razdor 120733505, original 18883840, the original one draw ahead; its
  trace: `Random(3000)` from 0x4ad933 at the end of the wait.
- The original draws `Random(3000)` for every idle patroller each time the hero stops
  (0x4ad8a0, the armies' idle-animation offset); Razdor has no such draw, so its stream falls
  behind by one draw per idle patroller per stop and every later roll differs.
- Spec: engine.md §3.4, row 0x4ad8a0 ("each time the hero stops").

## 2. A village entered while an event fires: offers, chords and tribute come too early

**Status: fixed in 0da6331** (`Game::enter_waiting_building`; world.md §7.2). The tribute is
still taken as the village window opens, the original's when it closes (window timing).

- Step 2 (`click_map 40,32`: village 4, event 17 fires on arrival) and step 3 (`ok`).
- Original: the event window opens first (chord `Random(3)` at 0x4d1663, then the patroller
  draw of §1); only when it is closed (step 3) is the village entered: the offer rolls
  `Random(2), Random(3), Random(6)×3` (0x4bbb64…0x4bbc44), then the village window's chord
  (0x4d1287). The tribute (40 gold, 15 mana) is paid when the village window closes:
  `hero.gold` 100 at steps 2 and 3, 140 at step 4.
- Razdor: at arrival the offer rolls (rules/economy.rs:815), then the village window's chord
  and the event's chord (rules/game.rs:879), and the tribute: `hero.gold` 140 already at
  step 2. Step 2: `rng` Razdor 3601754070, original 2785918235 (Razdor 5 draws ahead; the n
  agree for the first 32 draws, the AI's). So the offer rolls take other values than the
  original's.
- Spec: economy.md §3 "Entering a village" (the chooser runs when the village is entered,
  0x4bbc84, after the event scan; taking or closing the window pays the stocks);
  engine.md §3.4 (0x4bba40; chords 0x4d1282, 0x4d155f, 0x4d165e).

## 3. The hero's starting formation: the original auto-arranges it at the map load

**Status: fixed in 6aa8933** (`Game::arrange_at_load`; saves-data.md §10.1).

- Step 18 (`click_map 36,23`, the battle with the ruins' garrison), `battle.sides[0]`
  (type, row, col): original knight (1, 1, 4), militia (4, 1, 3), hunter (19, 2, 4),
  novice (26, 2, 3); Razdor knight (1, 3, 4), militia (4, 3, 3), hunter (19, 1, 4),
  novice (26, 1, 3). Read live right after the load in the original's army grid (army
  +0x1630): front row militia c3, knight c4; back row novice c3, hunter c4; reserve empty.
- Razdor places the class unit and the preset troops reserve first (`Formation::
  new_unit_slot`, as AddUnit 0x495ce0 does), so the knight and the militia sit in the
  reserve and the battle-start fix moves the hunter and the novice into the empty front row.
  The original's map load then runs every army, the hero's included, through the battle-side
  round trip (0x49855c builds the side and auto-arranges it with 0x483b3c; 0x4988c0 writes
  the grid back), so the hero's army starts in the auto-arranged formation (the best
  warrior in front column 4, the non-warriors in the back row, the other warriors in front).
- Consequence in this run: Razdor's front-row hunter and novice take the first blows and
  Razdor loses (step 32) the battle the original wins (step 36).
- Spec: battle.md "Where a new unit lands" says the starting units at map load go through
  AddUnit (reserve first) and stops there; it misses the load's round trip and
  auto-arrange. saves-data.md (map load) should list that step.

## 4. The ruins' garrison does not wear the ruins' goods

**Status: fixed in 080f7dd** (`ai::give_item_to` at the load, the loot takes worn items
first; economy.md §3).

- Step 18, the robber (type 66) of ruins 8: shot defence 9 in the original's battle record
  (+0x3c; base 5 plus the Round shield, item 51, `d-DefenceShot=4`), 5 in Razdor (building
  defence 3 on both sides). Step 21 (`battle_act 2,1,4`, the hunter's shot, attack 20):
  `battle.sides[1][0].hp` 57 in the original (20 − 12 = 8), 53 in Razdor (20 − 8 = 12).
- The original gives a ruins' first five goods to its garrison at load (worn or packed,
  0x4b554e, 0x4a273c); Razdor keeps them as the ruins' treasure only.
- Spec: economy.md §3 "Loot after the player's win", ruins ("their first 5 goods become
  garrison items, worn or packed").

## 5. AI armies arrive by play time and by frame, not as soon as the bank covers a step

**Status: the step-14 part fixed in f6f51f2** (`Game::ai_move` plays each tick by the step
clock's play times, arrivals in time order, a midnight among them; world.md §5). The frame
part stays noise: Razdor plays the order of the limit of short frames, so a run of the
original whose coarse frames cost an army a step, or put a midnight after a tick's last
arrivals, still differs there (seen again in fix runs 2, 3 and 4 at steps 14 and 16).

Traced with the Frida trace (`--trace random,ai,events`, run `rk1-frida`; README.md
"Runtime trace"). Two parts.

**Step 9, army 1 one cell off: frame noise of the original, not a rule.** The `x` 6 (Razdor)
against 7 (original) of the first run does not come back: the original gives 6 in the full
run `rk1-frida`, in four more runs of steps 0-11 and in two earlier runs, the same state as
Razdor. Between `rk1-day1` and `rk1-frida` the original's states differ only in
`armies[id 1].x` at steps 9-11 (7 / 6) and `armies[id 9].x` at steps 14-15 (16 / 15); the
generator agrees at every one of the 44 steps. The trace of army 1 at step 9: the step clock
(0x4a399c) arrives at (7,10) at t=38340 with 660 centi-minutes of the hero's step left; the
next call (t=39000, dt 165, the step's last frame) starts the step to (6,10) with the rest of
the window as its play time and arrives; then event 7 opens and the stop snaps the armies.
The step clock runs once per frame per army (World_AdvanceAI 0x4ade3c with the frame's
time); a call makes at most one arrival, the next step can only start at the next call, and
an arrival drops the rest of the frame's time (play time clamps to 0). So the cells an army
covers in a hero step depend on how the step's time falls into frames: when the arrival at
(7,10) lands in the last frame, the step to (6,10) waits for the next action (`rk1-day1`),
and as the minutes stay in the bank the army catches up later (step 12 equal again).
Reproduced: with a 25 ms or 60 ms sleep per frame (a hook on 0x4ade3c) army 1 stops at
x = 9 at step 9 and the generator then differs. Razdor's whole-minute model matches the
original at its normal pace; a one-cell difference at a stop can be this noise, and a run of
the original that is slowed (heavy hooks, a loaded machine) can show it.

**Step 14 on: arrivals within a tick come in another order.** Step-local, both sides start
step 14 from the state 108516897. Original: army 9's wander points (`Random(40)` ×8) at
t=67260, army 1's (`Random(17)/Random(14)`) at 69000 and again at 78000; Razdor: army 1's
first, then army 9's twice. The trace: army 1 stands at (6,6), bank 1250; its next step is
diagonal, 5250 (3500 × 1.5). First wait tick: bank 4250, no step. Second tick (t=66180):
bank 7250, the step starts, the 2000 left do not cover the following step, so its play
time is the rest of the window and it arrives at the end of the tick (69000), where its path
ends and it draws wander points. Army 9 arrives in the middle of that tick (67260) and draws
first. Razdor moves an army as soon as its bank covers a step, at the start of the slice and
army by army in index order (`rules/ai.rs` `ai_walk`), so army 1 arrives at 66000 and draws
first. The other draw order gives other wander points, the armies drift apart, and the
midnight restock of market 5 (step 16) starts from another state.
- Spec: world.md §5 (the play time: the cost scaled by tick / bank when the bank also covers
  the following step, else the rest of the window); it and ai.md §2 do not say that the step
  clock makes at most one arrival per call and frame and drops the rest of the frame's time.

## 6. Battle XP at the video's rate: 33 in Razdor, 16 in the original

**Status: deliberate, not changed** (Razdor's `content::PLAYER_XP_MODIFICATOR`, the user's
choice of 2026-09-29; experience.md §3 "The video's rate"). `known.py` classes it `known:6`.

- rk1-day1 step 36 (the ruins' battle won): `hero.units[k].xp` Razdor 33, 33, 33, 25;
  original 16, 16, 16, 12.
- Frida on 0x48bb10 (the battle's end with the flag) and its sides 0x669df8 / 0x66a64c: the
  awards (+0xa0 of the battle units) are 33, 33, 33, 25 in the original too, from the same
  pool (132: predicted loss 69, lost 67, the enemy's start strength 410, largest turn loss
  41) and the same useful / taken / left counts. The pre-battle prediction (0x48b75c) gave
  69 and 205 for the two sides and drew nothing.
- The payout (0x4c50ec, Community hook c2518f) multiplies by `HeroExpirienceModificator`
  (0x4ed3f0 = 50, the install's `_Global.ini`), the difficulty factor F (0x68e784 = 100) and
  the correction 100, over 10⁶: 33 × 0.5 = 16.5 → 16, 25 × 0.5 = 12.5 → 12 (halves to even).
  Razdor plays with 100 whatever the install says, by the user's choice after the gameplay
  video showed shares paid in full. Everything else in the XP chain matches; to match the
  Community Update's install, the constant would follow `_Global.ini` instead.

## 7. The battle is written back into the armies after every action

**Status: fixed** (`Game::battle_write_back`; battle.md §11).

- rk1-day1 steps 22-35 (the ruins' battle), step-local: `hero.units[0].hp` Razdor 80,
  original 63 at step 22 (51 at 24, 50 at 28, 49 at 34); `hero.units[1].hp` 50 against 38.
  The battle's own cards (`battle.sides`) agree on both sides; only the army record differs,
  and from step 36 (the battle's end) on it agrees again.
- The original's battle screen copies the sides out (0x48bb10 with flag 0) and writes them
  into both army records (0x4988c0) after every action: the player's (0x4c4f8c), each of the
  enemy's (0x4c57bc) and the end of a card's move (0x4b0284). The army record's unit HP
  (+0x20) follows the battle; the memory reader reads it there. Razdor wrote the battle back
  only at its end.
- Razdor now writes every fighter's HP into its army after each action (the hero's squad,
  and the enemy army's or garrison's troop records). The grid is still written at the end
  only: nothing reads it during the battle.

## 8. A village crossed on the way stopped Razdor's walk

**Status: fixed** (the capture window removed: `ui/world_view.rs`, `difftest.rs`; world.md
§4.2). Candidate C1003-173909.

- ДС1, step 5 (`click_map 96 18`), step-local: Razdor's hero stops at (95,15) after 98
  minutes, the original's walks on to (96,18) (151 minutes) and enters the village (offer
  rolls, the window's chord, the idle draws).
- (95,15) is a cell of that village. Stepping on an unguarded village takes it (0x4ad94c:
  owner, attitude and faction set) and the step goes on; no window opens. A Frida trace of
  the hero's cell (0x497c68) and his step check (0x4ad94c, which returned 0 at every step)
  shows the walk through (95,15), (96,16), (96,17) to (96,18) with no stop. Razdor showed
  its own "a new stronghold" window for every capture on the way, and the window held the
  walk (in the game and in the replay).
- After the fix the step's 66 draws all agree with the original's; left is the village's
  tribute, which the original pays when its window closes (window timing, README).

## 9. The first step at sea takes no time

**Status: fixed** (`Game::step_base`; world.md §2.1). Candidate C1003-175950.

- Тихая пристань (the hero starts on a ship at (48,1)), step 3 (`click_map 48 5`, four
  shallow-water steps south), step-local: `clock` Razdor 700069541, original 700069531
  (40 minutes against 30); Razdor's armies then arrive where the original's hero has
  stopped (the original makes its stop's idle draws, Razdor more wander points).
- Frida on 0x497c68 (the hero comes onto a cell): at the map load it sets the step time to
  0 (cost 0, at-sea flag 1 after the call); each later arrival sets 1000 centi-minutes
  (shallows, cost 2 × speed 5 × 100). The step from (48,1) took no game time.
- 0x497c68 reads the cell's cost on the map the at-sea flag (0x75bfdc) chooses, then sets
  the flag from the cell's terrain (0x496d28). At the map load the flag is still 0, so the
  start cell is priced on LAND, where water costs 0. The same holds for every boarding: the
  first water cell is priced on LAND and the step after it is free. Razdor priced every step
  when it was taken, with the flag as it stood then.
- After the fix all four steps of the repro agree with the original (the 27 draws of
  step 3 too).

## 10. An army's first step in place: the step after is priced south of it

**Status: fixed** (`AiMind::stand_facing`; ai.md §2). Candidate C1003-174927, first part.

- Проклятое озеро, step 2 (`wait 4`), step-local: the first 119 draws of the wait agree,
  then the original draws army 21's wander points (`Random(5)`) where Razdor draws army
  13's (`Random(11)`).
- Frida (`ai` preset): at the wait's first frame every army steps in place (no path,
  direction 5 from the map load). Army 13 stands on a cell of cost 4 (2000 centi-minutes,
  bank left 1000) and plays the step in 2000, so it arrives in the frame of t = 2020;
  Razdor gave it the whole window (3000) and it arrived with armies 18 and 21 at t = 1580.
- The step clock (0x4a399c) prices "the step after" on `cell + offset(+0x1710)`, the cell
  one step along the army's direction, not on the cell it stands on. The map load writes 5
  (south, offset (0, 1); tables 0x4ecf8c / 0x4ecfb0) into every record; every arrival or
  plan sets the direction from the path, 8 (no offset) without a next cell. So a fresh
  army's first step in place compares its bank with the cost of the cell south of it.
  Razdor priced its own cell.
- After the fix the wait's first 367 draws agree (§11-§13 explain the rest).

## 11. The AI's simulated battles count side strengths, not hit points

**Status: fixed** (`ai::simulate`; ai.md §4). Candidate C1003-174927, second part.

- After §10, Проклятое озеро step 2: army 9 re-plans at t = 9000 from (20,58); the
  original's path turns through (17,59), Razdor's through (17,58).
- Frida on the flood (0x482a58) of that plan: the multiplier map has the repulsion cone of
  army 2, a danger of strength 17 in the original; and on 0x4a08f8 (the army score) with
  0xc08998 / 0xc08994 / 0xc091ec / 0xc091e8 read at its end: army 9 against army 2 at the
  load, A0 = 296, B0 = 877. Army 9 has 240 HP (five units), army 2 322: these are the side
  strengths (+0x7ec of the sides 0xc081ac / 0xc08a00, written by 483ecc in the end's
  0x48bb10), not hit points. Razdor's `simulate` returned HP totals, so its scores, the
  cones and the seeds were off.
- After the fix B0 agrees with the original for every pair the traces show (877, 2536, 402,
  212, 4035, 1764); A0 needs §12, and the wait's first 415 draws agree.

## 12. An army's unit strengths keep the defence of their last recount

**Status: fixed** (`AiMind::strength_bd`, `Battle::set_strength_defence`; ai.md §4).
Candidate C1003-174927, third part.

- After §11 the side strengths still differed: army 9's A0 296 in the original, 312 in
  Razdor; army 2's B0 at the load 877 in the original, 1378 in Razdor (1764 on both sides
  later).
- Frida on the sims' set-up (0x48b75c, the battle units' +0x64 strengths, rows and the
  sides' +0x844 defence): army 2's units had strengths 299, 63, 63, 40, 84, 156 at the load
  and 486, 96, 96, 56, 84, 220 at t = 1620, the same units at full HP. The battle unit's
  strength is the army record's cached +0x1ae, which only the recount 0x4a16d4 writes,
  with the army's building defence (+0x378c) at that moment; the map load's set-up
  (0x4a1ff0) recounts before it writes +0x378c. Army 2 starts in a building of defence 15
  and army 9 on one of defence 2: both were counted with 0 until a recount (army 2's first
  arrival in its building; army 9 still counted 296 at t = 1620 after arriving on its cell
  again, an arrival that ends the rules before the recount, as on a bridge). Razdor worked
  out every strength afresh with the current defence.
- After the fix A0 and B0 agree with every value the traces show.

## 13. A negative aggression's tenth applies only when the side lost no unit

**Status: fixed** (`SimResult::own_lost_units`, `ai::army_score`; ai.md §4). Candidate
C1003-174927, fourth part: with it the candidate is resolved.

- After §12, Проклятое озеро step 2: army 2's plan at t = 7240 goes for another target;
  Frida on its flood (0x482a58) shows other seed values for armies 21 and 22 (2235 against
  Razdor's 1673) and others.
- Frida on 0x4a08f8 for army 2 (aggression −25) with the four results read at its end:
  against army 3, A0 877 and A1 141 = 360 + Round(−25 × 877 / 100); against army 9, A1 832
  = 854 + Round(−25 × 877 / 1000). The ÷1000 applies when the side record's first word (its
  living count after the end's copy, 0xc081ac) is not below +4 (its start count, 0xc081b0),
  i.e. when the scoring side lost no unit; ai.md had that value as unknown and Razdor took
  ÷1000 always.
- After the fix the wait's 509 draws all agree with the original's and every field of the
  step is equal.

## 14. The victory box draws the event window's chord

**Status: fixed** (the replay's and the interface's victory dialog draw it; engine.md §3.4).

- rk1-day1 step 36 (the blow that ends the ruins' battle), step-local: the original draws
  one `Random(3)` from 0x4d1663 (the return of the event window's chord call at 0x4d165e);
  Razdor none. The original's screen shows "Победа над врагом!" in the event window (the
  memory reader's `screen` is "event").
- The victory box of the player's battle is the event/reward dialog (0x672618, opened by
  0x4d15d0), which draws its chord as any event's. Razdor drew the chord only for the
  scenario's events.
- After the fix steps 36-39 of rk1-day1 are equal step-local.

## Not differences

- **Events queued behind the window on screen** (candidate C1003-174531, Обучающий1 step 5):
  arriving at (22,41) fires events 4 and 5; the original shows event 4's window and fires
  event 5 when it is closed (its window next), Razdor counts both at once. With three `ok`
  after it the run is equal at every step, generator included (run `r2-c174531-ext`): only
  window timing. `known.py` classes such an `events_done` difference `timing`.

- **Events done** and **event results while the window is up**: the original counts an event
  and applies its finishing results (an army switched off: step 9 `armies[id 14].active`)
  when its window is closed (events.md §6.1, §7.2); Razdor at once. Same order of events
  (7, then 6). The differ counts the shown event as done and reports such fields as window
  timing.
- **Unit level** (0 against 1): fixed in `razdor --replay` (the state numbers levels from 0,
  as the map file and the unit record do).
- **Music**: with the timed change held off, the only music draws seen are the game's own
  (`Random(8), Random(60000)` when the victory report closes, step 38).
