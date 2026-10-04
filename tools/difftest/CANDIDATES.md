# Diff test candidates

Differences the LLM explorer (`explore.py`) found that `known.py` could not match to a FINDINGS.md entry or to noise. Each needs a human (or Claude) to confirm it, explain it and move it to FINDINGS.md, or teach `known.py` to recognise it.
# Diff test candidates

Differences the LLM explorer (`explore.py`) found that `known.py` could not match to a FINDINGS.md entry or to noise. Each needs a human (or Claude) to confirm it, explain it and move it to FINDINGS.md, or teach `known.py` to recognise it.

## C1003-173909: ДС1-С чего все начиналось, step 5 `click_map 96 18`

- Found 2026-10-03 by explore.py (hero 1); unconfirmed.
- Repro: 6 actions (shrunk from 10, 1 tries): `~/.cache/razdor-difftest/explore/C1003-173909/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `clock`: 98 / 151
  - `hero.x`: 95 / 96
  - `hero.y`: 15 / 18
- rng: the original is 2 draw(s) ahead of Razdor
- rng: draws this step: Razdor 64, original 66; the n agree for the first 0
- rng: original from there: Random(23) AI wander points 0x4a2594, Random(23) AI wander points 0x4a25d8, Random(23) AI wander points 0x4a2594, Random(23) AI wander points 0x4a25d8, Random(23) AI wander points 0x4a2594, Random(23) AI wander points 0x4a25d8, Random(23) AI wander points 0x4a2594, Random(23) AI wander points 0x4a25d8 ...
- rng: Razdor from there: Random(100) rules/ai.rs:1465, Random(100) rules/ai.rs:1466, Random(100) rules/ai.rs:1465, Random(100) rules/ai.rs:1466, Random(100) rules/ai.rs:1465, Random(100) rules/ai.rs:1466, Random(100) rules/ai.rs:1465, Random(100) rules/ai.rs:1466 ...
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1003-173909/`.
- Re-run after the FINDINGS §1–§5 fixes (f6f51f2): **not resolved**. Razdor's walk still stops at (95,15) after 98 minutes where the original walks on to the village at (96,18) (151) and enters it (offer rolls, chord, idle draws); the step's first 56 draws now agree. Another cause than §1–§5.
- Second round (run `r2-fix-C1003-173909`): **resolved** by FINDINGS §8 (6fa6992): a
  village taken on the way opened Razdor's own capture window, which held the walk; the
  original opens none. The step's 66 draws agree; left is the village's tribute, paid when
  its window closes in the original (window timing; the repro ends with the window open).

## C1003-174531: Обучающий1, step 5 `click_map 22 41`

- Found 2026-10-03 by explore.py (hero 1); unconfirmed.
- Repro: 6 actions (shrunk from 6, 0 tries): `~/.cache/razdor-difftest/explore/C1003-174531/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `events_done`: [1, 2, 3, 4, 5] / [1, 2, 3, 4]
- rng: the original is 1 draw(s) ahead of Razdor
- rng: draws this step: Razdor 33, original 34; the n agree for the first 33
- rng: original from there: Random(3000) patroller idle offset 0x4ad933
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1003-174531/`.
- Re-run after the fixes: the generator difference (the missing `Random(3000)`) is **resolved by §1** (1c1696f). Left: `events_done` [1..5] against [1..4] with event 4 on screen, likely the window timing of a second event queued behind the first (not confirmed).
- Second round: **window timing only, confirmed**. With three `ok` after the repro (run
  `r2-c174531-ext`) every step is equal, generator included: event 5 fires in the original
  when event 4's window closes. `known.py` now classes it `timing` (FINDINGS "Not
  differences").

## C1003-174927: Проклятое озеро, step 2 `wait 4`

- Found 2026-10-03 by explore.py (hero 1); unconfirmed.
- Repro: 3 actions (shrunk from 3, 0 tries): `~/.cache/razdor-difftest/explore/C1003-174927/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `rng`: 2023555501 / 996532122
  - `armies[id 10].x`: 80 / 74
  - `armies[id 10].y`: 47 / 39
  - `armies[id 11].x`: 48 / 42
  - `armies[id 11].y`: 22 / 29
  - `armies[id 12].x`: 58 / 50
  - `armies[id 12].y`: 77 / 75
  - `armies[id 14].y`: 77 / 80
  - ... 43 more
- rng: Razdor is 25 draw(s) ahead of the original
- rng: draws this step: Razdor 534, original 509; the n agree for the first 8
- rng: original from there: Random(79) ? 0x4a6b74, Random(3) AI promotion 0x4a4b00, Random(100) AI wander points 0x4a2624, Random(100) AI wander points 0x4a264d, Random(100) AI wander points 0x4a2624, Random(100) AI wander points 0x4a264d, Random(100) AI wander points 0x4a2624, Random(100) AI wander points 0x4a264d ...
- rng: Razdor from there: Random(7) rules/ai.rs:2162, Random(54) rules/ai.rs:2162, Random(100) rules/ai.rs:1468, Random(100) rules/ai.rs:1469, Random(100) rules/ai.rs:1468, Random(100) rules/ai.rs:1469, Random(100) rules/ai.rs:1468, Random(100) rules/ai.rs:1469 ...
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1003-174927/`.
- Re-run after the fixes: **not resolved**, though the first 119 draws of the wait now agree (8 before). The armies' plans part after that (the original draws `Random(5)` wander points where Razdor draws `Random(11)`): another cause than §1–§5.
- Second round (run `r2-fix-C1003-174927`): **resolved**, every step equal. Four causes,
  FINDINGS §10-§13: the first step in place prices the step after south of the army (the
  load's direction 5, 9301b1b); simulated battles count side strengths, not HP (d9eec17);
  the strengths keep the building defence of the army's last recount (768ec5d); the
  negative aggression's tenth only when no unit was lost (3688b16). The "?" caller 0x4a6b74
  is the AI hire XP roll inside 0x4a548c (named in memread.py and engine.md §3.4).

## C1003-175950: Тихая пристань, step 3 `click_map 48 5`

- Found 2026-10-03 by explore.py (hero 1); unconfirmed.
- Repro: 4 actions (shrunk from 4, 0 tries): `~/.cache/razdor-difftest/explore/C1003-175950/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `clock`: 700069541 / 700069531
- rng: Razdor is 13 draw(s) ahead of the original
- rng: draws this step: Razdor 40, original 27; the n agree for the first 24
- rng: original from there: Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933
- rng: Razdor from there: Random(11) rules/ai.rs:1465, Random(11) rules/ai.rs:1466, Random(11) rules/ai.rs:1465, Random(11) rules/ai.rs:1466, Random(11) rules/ai.rs:1465, Random(11) rules/ai.rs:1466, Random(11) rules/ai.rs:1465, Random(11) rules/ai.rs:1466 ...
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1003-175950/`.
- Re-run after the fixes: **not resolved**. The original's walk ends 10 minutes earlier (`clock`) and makes the stop's idle draws where Razdor's walk goes on and its armies draw wander points: a walk-end difference, another cause than §1–§5.
- Second round (run `r2-fix-C1003-175950`): **resolved**, every step equal: the hero's step
  time is set as he comes onto a cell, priced with the at-sea flag before it, so the first
  step from a start on the water is free (FINDINGS §9, c49d28e).

# Third round: the explorer with services (run of 2026-10-03 23:13, one hour)

## C1003-231958: ДС1-С чего все начиналось, step 5 `click_map 96 18`

- Found 2026-10-03 by explore.py (hero 1); unconfirmed.
- Repro: 6 actions (shrunk from 6, 0 tries): `~/.cache/razdor-difftest/explore/C1003-231958/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `buildings[id 13].gold`: 0 / 50
  - `buildings[id 13].mana`: 0 / 40
  - `hero.gold`: 1050 / 1000
  - `hero.mana`: 40 / 0
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1003-231958/`.
- **Window timing, not a difference**: at the step the original's village window is open;
  it pays the tribute when the window closes, Razdor on entering. `known.py` now classes the
  hero's and the village's gold and mana `timing` while the original shows a village window.

## C1003-232646: Другой берег, step 17 `click_map 60 50`

- Found 2026-10-03 by explore.py (hero 2); unconfirmed.
- Repro: 18 actions (shrunk from 19, 3 tries): `~/.cache/razdor-difftest/explore/C1003-232646/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `hero.gold`: 185 / 100
  - `hero.mana`: 355 / 320
- rng: Razdor is 1 draw(s) ahead of the original
- rng: draws this step: Razdor 83, original 82; the n agree for the first 82
- rng: Razdor from there: Random(3000) rules/ai.rs:1418
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1003-232646/`.
- **Window timing** as C1003-231958 (the original shows the village window at step 17); the
  extra `Random(3000)` is FINDINGS §1's patroller draw at the stop. Classed `timing` now.

## C1003-234059: Проклятое озеро, step 4 `click_map 6 6`

- Found 2026-10-03 by explore.py (hero 1); unconfirmed.
- Repro: 5 actions (shrunk from 8, 3 tries): `~/.cache/razdor-difftest/explore/C1003-234059/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `rng`: 1090191167 / 1574751335
  - `armies[id 15].y`: 37 / 36
  - `armies[id 16].x`: 46 / 43
  - `armies[id 16].y`: 16 / 18
  - `armies[id 18].y`: 33 / 32
  - `armies[id 22].x`: 59 / 60
  - `armies[id 24].y`: 19 / 18
  - `armies[id 26].y`: 58 / 57
- rng: the original is 8 draw(s) ahead of Razdor
- rng: draws this step: Razdor 304, original 312; the n agree for the first 120
- rng: original from there: Random(54) AI hire XP 0x4a6b74, Random(13) AI wander points 0x4a2594, Random(13) AI wander points 0x4a25d8, Random(13) AI wander points 0x4a2594, Random(13) AI wander points 0x4a25d8, Random(13) AI wander points 0x4a2594, Random(13) AI wander points 0x4a25d8, Random(13) AI wander points 0x4a2594 ...
- rng: Razdor from there: Random(11) rules/ai.rs:1664, Random(11) rules/ai.rs:1665, Random(11) rules/ai.rs:1664, Random(11) rules/ai.rs:1665, Random(11) rules/ai.rs:1664, Random(11) rules/ai.rs:1665, Random(11) rules/ai.rs:1664, Random(11) rules/ai.rs:1665 ...
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1003-234059/`.
- Reading: the first 120 draws agree; then the original draws `Random(54)` at 0x4a6b74, the
  XP of a unit an AI army hires on arriving in a building (inside the arrival rules
  0x4a548c), where Razdor goes on with wander points. An AI hire that Razdor does not make
  at that arrival (or makes without the XP roll); every army's plan after it differs.
- **Explained, frame order** (third fix round, run `r3-c234059`; FINDINGS §5, third part):
  Razdor makes the same hire and the same `Random(54)` (army 2, a militia at its castle);
  the original runs the arrivals of one long frame (360 centi-minutes) in index order, Razdor
  by their exact times, so army 26's wander points come first in Razdor. `known.py` classes
  it §5 now.

## C1003-235033: РК1-Начало пути, step 7 `click_map 31 43`

- Found 2026-10-03 by explore.py (hero 2); unconfirmed.
- Repro: 8 actions (shrunk from 13, 3 tries): `~/.cache/razdor-difftest/explore/C1003-235033/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `clock`: 624298221 / 624297973
  - `hero.x`: 38 / 46
  - `hero.y`: 16 / 25
- rng: Razdor is 176 draw(s) ahead of the original
- rng: draws this step: Razdor 187, original 11; the n agree for the first 0
- rng: original from there: Random(50) AI wander points 0x4a2624, Random(50) AI wander points 0x4a264d, Random(50) AI wander points 0x4a2624, Random(50) AI wander points 0x4a264d, Random(50) AI wander points 0x4a2624, Random(50) AI wander points 0x4a264d, Random(50) AI wander points 0x4a2624, Random(50) AI wander points 0x4a264d ...
- rng: Razdor from there: Random(40) rules/ai.rs:1664, Random(40) rules/ai.rs:1665, Random(40) rules/ai.rs:1664, Random(40) rules/ai.rs:1665, Random(40) rules/ai.rs:1664, Random(40) rules/ai.rs:1665, Random(40) rules/ai.rs:1664, Random(40) rules/ai.rs:1665 ...
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1003-235033/`.
- Reading: in the original the messenger (army 14) meets the hero 37 minutes into the walk
  from the church (event 7, a meeting) and stops it at (46,25); in Razdor the messenger is
  elsewhere and the hero walks on for 285 minutes. The two sides' first draws of the step
  are already wander points of different ranges (`Random(50)` against `Random(40)`): an
  army's wander box differs, so its plan, and the meeting, part.
- **Explained, frame order** (third fix round, run `r3-c235033` with the `ai` preset;
  FINDINGS §5, third part): the boxes agree on both sides (army 1 17 × 14, army 9 40 × 40,
  army 14 the whole map); the first differing draws are army 1's box against army 9's, in
  another order because the original's long frames during the walk delay the arrivals.

## Обучающий1 and Другой берег (ranger): the first step is priced at the knight's speed

- Found 2026-10-03 in the explorer's ranger episodes (ep1003-232647, ep1004-000803; classed
  `seen` there through an older signature, now per map); confirmed by run `cand-ranger-ghost`.
- Repro (5 actions): `new_game Обучающий1 hero 3`, three `ok`, `click_map 18 42`.
- Step 4: `clock` Razdor 313 minutes after the start, the original 306 (7 less); every other
  field equal, the generator too. On Другой берег (hero 3, `click_map 10 10` after two `ok`)
  the time-fired event stops Razdor's walk a cell earlier (1022 against 1033 minutes).
- Likely cause (not fixed here): `Game::unstarted` sets the first step's base
  (`land_step_base(start)`, cost × speed, FINDINGS §9) before `archetype` is set, so
  `hero_speed()` still answers the knight's 5 for a ranger (4): the first step costs
  cost × 5 instead of cost × 4 (7 minutes here, a start cell of cost 7).
- **Resolved** by FINDINGS §16 (third fix round): the cause confirmed in the exe (0x4b4300
  sets the speed before 0x4b5913 puts him on his cell); Обучающий1 9 of 9 steps equal,
  Другой берег 8 of 8, C1004-005130 6 of 6.

## Проклятое озеро: a village offer's roll is drawn when the offer opens, not at the answer

- Found 2026-10-03 (ep1003-232858, steps 2-3; the matcher took it for FINDINGS §2, whose
  rule only looks for Razdor's village draws while the original shows an event window).
- Repro: `new_game Проклятое озеро hero 1`, `ok`, `click_map 5 15` (the village offers),
  `answer yes`.
- Step 2: the original draws `Random(5)` at 0x4acb89 (VillageOffer_Build: the blessing's
  spell, or the witch's mana `300 + 50·Rand(5)`) and then the event window's chord; Razdor
  draws only the chord. Step 3 (yes): Razdor draws its `Random(5)` in `accept_offer`
  (rules/economy.rs), the original nothing. economy.md §3 gives the rolls of each option
  (spell 3 + 2·Rand(5), mana 300 + 50·Rand(5)) but not their moment; the trace puts it in
  the offer's build (0x4aca80), as the offer opens. (Razdor's blessing also rolls over the
  blessing spells the install has; with all five, `Random(5)` as the original.)
- **Resolved** by FINDINGS §17 (third fix round): the roll moved to the offer's opening;
  6 of 6 steps equal (run `r3-lake-offer`).

## Проклятое озеро: the first midnight restocks a market in the original only

- Found 2026-10-03 (ep1003-232858 step 7, after the step above; step-local, so independent
  of it); unconfirmed beyond that run.
- At the first midnight after the load both sides draw the barracks' regrowth
  (`Random(1)`, `Random(3)` pairs) in step; then the original draws a market restock
  (`Random(5)` at 0x4be2a4, then the goods' rolls) and Razdor goes on with the next
  barracks: Razdor's market is not due. economy.md §2: a market with random goods is
  stocked at the load and "after acting" its timer is set 12 hours on; the original
  restocks again at the first midnight, so the load's stocking seems not to set the timer.
  Razdor's midnight also restocks a building's market before its barracks, the original's
  draws show the barracks first: a second order to check.
- **Confirmed, frame-dependent** (third fix round, run `r3-ep232858`; FINDINGS §5, third
  part; economy.md §2): the load sets the timer to start + 1 + 720, one minute after the first
  midnight of a map that starts at noon; the original's midnight ran in a frame a minute past
  it and restocked, Razdor's runs at the midnight's minute. The second order is not one: the
  original restocks each building before its barracks (0x4a1998), as Razdor does; the draws
  before it were the earlier buildings' barracks.

## Not differences found on the way

- **A click on the village the hero stands in, its window open** (ДС1 step 21): the
  original's harness closes the window and the click opens it again, with the village
  window's chord; Razdor's replay did not count the reopening as a new window. Fixed in
  the replay (`src/difftest.rs`), checked on the recorded run (`ds1-reclick-check`).
- **An AI army's wander box** (ДС1 step 27: `Random(31)/Random(21)` in the original against
  `Random(100)` in Razdor): the matcher classes it FINDINGS §5 (arrival order); the ranges
  say an army wanders a patrol box in the original and the whole map in Razdor. Not
  followed up; the same kind of reading as C1003-235033.
  Third round: as C1003-235033, most likely two armies' draws in another order (one with a
  box, one with the whole map), not one army's area; `known.py` now classes such a step
  §5 only when each side's ranges are drawn by the other side too.

# Third round, second run: service goals only (2026-10-04 00:29, ДС1 and РК1, 0.47 h)

## C1004-003724: ДС1-С чего все начиналось, step 15 `click_map 83 27`

- Found 2026-10-04 by explore.py (hero 1); unconfirmed.
- Repro: 16 actions (shrunk from 16, 2 tries): `~/.cache/razdor-difftest/explore/C1004-003724/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `rng`: 2617033078 / 1491519599
- rng: Razdor is 1 draw(s) ahead of the original
- rng: draws this step: Razdor 41, original 40; the n agree for the first 39
- rng: original from there: Random(3) window chord 0x4d1663
- rng: Razdor from there: Random(3000) rules/ai.rs:1418, Random(3) rules/game.rs:942
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1004-003724/`.
- Reading: the walk stops at an event (25); both sides draw the patrollers' idle offsets
  (FINDINGS §1) and then the event window's chord, but Razdor draws one `Random(3000)`
  more: it counts one more idle patroller at that stop than the original. Which army is
  not traced yet (the `ai` preset of `trace.py` at the step names the armies).
- **Resolved** by FINDINGS §18 (third fix round): army 5, arriving at the end of the hero's
  step, sees him arrived and stands in the original; 16 of 16 steps equal (`r3-c003724`).

## C1004-004157: РК1-Начало пути, step 13 `answer true`

- Found 2026-10-04 by explore.py (hero 2); unconfirmed.
- Repro: 14 actions (shrunk from 23, 2 tries): `~/.cache/razdor-difftest/explore/C1004-004157/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `rng`: 2380015889 / 3746159078
- rng: Razdor is 1 draw(s) ahead of the original
- rng: draws this step: Razdor 1, original 0; the n agree for the first 0
- rng: Razdor from there: Random(5) rules/economy.rs:866
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1004-004157/`.
- **Same cause as "a village offer's roll is drawn when the offer opens"** above (the
  original's `Random(5)` at 0x4acb89 in step 12, before the chord; Razdor's at the yes):
  seen on a second map (РК1, the village at (40,32), a blessing). The offer itself played
  equal on both sides (the question, the yes, the blessing).
- **Resolved** by FINDINGS §17: 15 of 15 steps equal (run `r3-c004157`).

## C1004-005130: ДС1-С чего все начиналось, step 5 `click_map 97 7`

- Found 2026-10-04 by explore.py (hero 3); unconfirmed.
- Repro: 6 actions (shrunk from 6, 0 tries): `~/.cache/razdor-difftest/explore/C1004-005130/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `armies[id 3].x`: 47 / 54
  - `armies[id 3].y`: 57 / 70
  - `clock`: 64 / 61
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1004-005130/`.
- **The ranger's first step** above (hero 3; `clock` 3 minutes more in Razdor, a start
  cell of cost 3); the AI army 3 one step behind follows from the later stop.
- **Resolved** by FINDINGS §16: 6 of 6 steps equal (run `r3-c005130`).

## C1004-005736: РК1-Начало пути, step 20 `click_map 40 32`

- Found 2026-10-04 by explore.py (hero 1); unconfirmed.
- Repro: 21 actions (shrunk from 21, 2 tries): `~/.cache/razdor-difftest/explore/C1004-005736/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `events_done`: [1, 4, 5, 6, 7, 17] / [1, 4, 5, 17]
- rng: Razdor is 50 draw(s) ahead of the original
- rng: draws this step: Razdor 155, original 105; the n agree for the first 0
- rng: original from there: Random(17) AI wander points 0x4a2594, Random(14) AI wander points 0x4a25d8, Random(17) AI wander points 0x4a2594, Random(14) AI wander points 0x4a25d8, Random(17) AI wander points 0x4a2594, Random(14) AI wander points 0x4a25d8, Random(17) AI wander points 0x4a2594, Random(14) AI wander points 0x4a25d8 ...
- rng: Razdor from there: Random(40) rules/ai.rs:1664, Random(40) rules/ai.rs:1665, Random(40) rules/ai.rs:1664, Random(40) rules/ai.rs:1665, Random(40) rules/ai.rs:1664, Random(40) rules/ai.rs:1665, Random(40) rules/ai.rs:1664, Random(40) rules/ai.rs:1665 ...
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1004-005736/`.
- Reading: the messenger (events 6 and 7, the meeting with army 14) reaches the hero in
  Razdor and not yet in the original; the step's first draws are wander points of other
  ranges on each side (`Random(17)/Random(14)` against `Random(40)`). Same family as
  C1003-235033: an army's wander box.
- **Explained** with C1003-235033 (the same armies 1 and 9 of РК1; FINDINGS §5, third part).

# Fourth round: after fix round 3 (2026-10-04 03:45, all maps, 0.76 h, 5 episodes)

The explorer with the default model (Qwen3-Coder-30B-A3B) after FINDINGS §16-§19: 5 episodes,
309 actions, 3 NEW. Triage below each.

## C1004-035744: ДС1-С чего все начиналось, step 16 `click_map 97 7`

- Found 2026-10-04 by explore.py (hero 1); unconfirmed.
- Repro: 17 actions (shrunk from 17, 4 tries): `~/.cache/razdor-difftest/explore/C1004-035744/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `clock`: 2666 / 2936
  - `hero.units[0].hp`: 40 / 80
  - `hero.x`: 87 / 97
  - `hero.y`: 19 / 7
- rng: the original is 233 draw(s) ahead of Razdor
- rng: draws this step: Razdor 0, original 233; the n agree for the first 0
- rng: original from there: Random(89) AI wander points 0x4a2594, Random(80) AI wander points 0x4a25d8, Random(89) AI wander points 0x4a2594, Random(80) AI wander points 0x4a25d8, Random(89) AI wander points 0x4a2594, Random(80) AI wander points 0x4a25d8, Random(89) AI wander points 0x4a2594, Random(80) AI wander points 0x4a25d8 ...
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1004-035744/`.
- Reading (not traced to a cause): step-local, the generator equal before the step; at its
  start army 1 (the bandits, 4 units) stands at (91,17), the hero at (85,19) alone. In Razdor
  the army comes to (88,19) and attacks after two of his steps (40 minutes, no AI draw before
  the battle); in the original it ends at (92,13), draws wander points (`Random(89)/(80)`) and
  the hero walks on to (97,7) unattacked.
- **Cause (fifth round, traced): a Razdor rule bug, the hero's cell the AI sees during his
  step.** Step 16 is downstream: army 1 already stands elsewhere since step 10 (`click_map 86
  13`; the step-local run starts each step from Razdor's own armies, only the generator is
  the original's), where the original is 48 draws ahead (the first 230 agree, then the
  original draws army 1's wander points `Random(31)/Random(21)` at 159750 and Razdor army
  16's). At 137545 army 1 re-plans at (70,5) (both sides, same time within a frame). Its patrol box is
  x 65..95, y 0..20 (`+0x16c0..`, read live). The hero walks (98,1) → (86,13); his step
  (96,3) → (95,4) runs 1353.5–1421 minutes. A Frida hook on the planner 0x4a2d88 (army 1)
  reads the hero's record cell (`army(0)+0x1724`) = **(96,3)**, the cell he leaves, outside
  the box, so the original seeds no hero (score[1][0] = 5, clean) and keeps its path to the
  wander point (65,3). Razdor's planner takes the hero at **(95,4)**, the cell he steps to,
  inside the box, seeds him (score 5, the cheapest seed) and turns army 1 east toward him;
  from there army 1 hunts him and at step 16 attacks him.
  - The rule (world.md §5, ai.md §7.5): during a hero step his *logical* cell is the cell he
    leaves; it becomes the new cell only in the frame the step ends (0x4ae8cc), before the
    armies advance. So everything the AI computes from "the hero's cell" between a step's
    start and its end uses the cell left: the distances of `ai_arrival` (the replan within
    `AIGetPathDistance`, the talk counts), the rescoring range, the seed cell and the
    patrol-box test of `ai_plan`, the erase of his cell. At the tick's end (an army arriving
    in the step's last frame, `hero_end`) it is the new cell (as commit 4a41d9e has it).
  - Where: `src/rules/ai.rs` `Game::cell_of` (`Party::Hero => self.tile()`), called by
    `ai_arrival` and `ai_plan`; `move_armies` (`src/rules/game.rs`) moves the hero to the
    new tile before `ai_move`, and `HeroCells` already carries `step_from` (`cells[1]`
    while stepping) and the end-of-tick cells (`hero_end`). Check `ai_arrive_rules` too
    (adjacency for attacks and greetings reads the same logical cell; the "cell plus
    direction" entry test is already handled through `HeroCells`).
  - Checked with a throw-away instrumented build (not committed): with the hero's cell taken
    as `step_from` in the AI's arrivals of a step (not at the tick's end), step 10 of this
    run comes out equal (generator 2236314233, army 1 at (68,5) on both sides).

## C1004-041105: Другой берег, step 17 `click_map 81 77`

- Found 2026-10-04 by explore.py (hero 2); unconfirmed.
- Repro: 18 actions (shrunk from 29, 4 tries): `~/.cache/razdor-difftest/explore/C1004-041105/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `rng`: 3113229542 / 2044507713
  - `armies[id 10].x`: 162 / 158
  - `armies[id 11].x`: 10 / 8
  - `armies[id 11].y`: 14 / 11
  - `armies[id 13].y`: 61 / 60
  - `armies[id 14].x`: 30 / 31
  - `armies[id 14].y`: 70 / 69
  - `armies[id 15].x`: 191 / 187
  - ... 40 more
- rng: Razdor is 15 draw(s) ahead of the original
- rng: draws this step: Razdor 191, original 176; the n agree for the first 153
- rng: original from there: Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933, Random(3000) patroller idle offset 0x4ad933 ...
- rng: Razdor from there: Random(55) rules/ai.rs:1686, Random(55) rules/ai.rs:1687, Random(55) rules/ai.rs:1686, Random(55) rules/ai.rs:1687, Random(55) rules/ai.rs:1686, Random(55) rules/ai.rs:1687, Random(55) rules/ai.rs:1686, Random(55) rules/ai.rs:1687 ...
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1004-041105/`.
- Reading (not traced): the original's walk stops and draws the stop's idle offsets where
  Razdor's armies go on (army wander points, `Random(55)`), so the walk ends earlier in the
  original (`clock`); the first 153 draws agree.
- **Cause (fifth round, traced): a Razdor rule bug, the hero's side strength in an AI army's
  score.** The click (81,77) is on army 36 ("Беглые крестьяне #1"), so the hero chases it;
  both sides end the chase when the army steps into the dark (an empty re-plan), the
  original at (78,66) (585 min), Razdor at (80,68) (646). Army 36 walks another way: in the
  original (84,72) → (84,73) → (85,74) → (86,75), in Razdor (84,72) → (85,73) → (85,74) →
  (86,75), later. Its plan at 40500 rescored the hero: original −17, Razdor −16 (a danger,
  so a stronger repulsion cone in the original). A Frida hook on AI_ArmyTargetScore
  0x4a08f8 (army 36 against 0) reads A0 531 on both sides but **B0 = 928** (the hero's side)
  against Razdor's 624.
  - The rule (ai.md §4 "the unit strengths are the army's cached ones", now for the
    player): the hero's side copies each unit's cached strength `+0x1ae` from army record 0,
    which only the recount 0x4a16d4 writes, with the building defence `army(0)+0x378c` of
    that moment. The hero's recounts here run through 0x497240 (return 0x497256) and the
    hire (0x4bd5e7), all inside the town (defence 15): the units' `+0x1ae` = 413, 134, 96,
    220, 96, 220 (without defence, `+0x1aa`: 294, 86, 63, 156, 63, 156). Walking out clears
    `+0x378c` to 0 but does not recount, so at 40500 in the open his side still counts the
    town's defence 15.
  - Where: `src/rules/ai.rs` `Game::hero_side` builds `Side { units, defence,
    strength_defence: defence }` with `defence = hero_defence()` (the building he stands in
    now). `strength_defence` should be the defence of the hero's last recount (the same idea
    as `AiMind::strength_bd` for the AI armies): set where the original calls 0x4a16d4 for
    army 0 (0x497240's call sites: his battles, his noon, an event that took effect, a
    building window 0x4ba854; the hire 0x4bd5e7; the load), not cleared when he walks out.
    The battle's own `defence` stays the current one.
  - Checked with a throw-away instrumented build (not committed): with the hero's
    `strength_defence` forced to 15 the step comes out equal (clock 586, hero (78,66),
    generator 2044507713 as the original).
  - Not the cause here but seen on the way: the chase re-plans whenever the chased army has
    arrived since the hero's last step in Razdor, while the original re-plans only when the
    army ends a step in the frame where the hero ends one (world.md §1.3). With equal army
    steps it gave the same targets here.

## C1004-042357: Обучающий1, step 17 `click_map 20 28`

- Found 2026-10-04 by explore.py (hero 3); unconfirmed.
- Repro: 18 actions (shrunk from 58, 4 tries): `~/.cache/razdor-difftest/explore/C1004-042357/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `events_done`: [1, 2, 3, 4, 5, 9] / [1, 2, 3, 4, 5, 6, 9]
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1004-042357/`.
- **Harness, not a difference**: event 6 is a yes/no question on both sides at the step
  (Razdor's screen `question`, event 6); the differ counts the original's shown event as
  done, Razdor counts a question only once it is answered. `known.py` classes it `timing`
  now.

## lake-gang.jsonl step 3 (`click_map 19 18`): army 7 seven cells off, generator equal

- Found 2026-10-04 in the gang-gold run (`r3-gold-lake17`), again in `r3-army7` (`ai:7`).
- Step-local: `armies[id 7]` (46,85) in Razdor, (39,92) in the original; nothing else differs.
- Reading: **§5 frames**. Army 7 arrives at (45,92) at 22300 centi-minutes in Razdor, at
  22400 in the original (the frame's end, 100 later); its plan there reads a flood whose
  seeds (other armies' cells) differ by that minute, and turns north toward a friendly army
  at (49,82) in Razdor, west toward (34,84) in the original. No draw in between, so the
  generator stays equal. Larger than the one-cell `noise` `known.py` knows; not fixed.

# Fifth round: forced coverage (`explore.py --cover`, 2026-10-04)

## C1004-050909: ДС1-С чего все начиналось, step 7 `buy 8`

- Found 2026-10-04 by explore.py (hero 1); unconfirmed.
- Repro: 8 actions (shrunk from 8, 0 tries): `~/.cache/razdor-difftest/explore/C1004-050909/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.
- Step-local run, fields classed NEW (Razdor / original):
  - `buildings[id 11].goods`: [73, 74, 75, 95, 76, 76, 77, 99] / [73, 74, 75, 95, 76, 76, 77, 99, 100]
- The original gave the same values on a second run (trace `random,ai,events`). Files: states, screenshots, `trace-around.jsonl` in `~/.cache/razdor-difftest/explore/C1004-050909/`.
- **Harness timing, not a difference.** Both sides bought item 100 for 25 gold (pack [100]
  on both). The original empties the market slot in the window's own list and writes it back
  to the building record only when the window closes (economy.md, player market, 0x4b9e18);
  the state is read with the window still open, so the building's `+0x88` words still hold
  100. `known.py` classes a market's goods `timing` while the original's building window
  stays open after a `buy` (Razdor's list must be the original's minus the bought items).

