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
