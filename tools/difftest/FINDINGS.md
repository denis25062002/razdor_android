# Diff test findings

Differences between Razdor and the original Discord Times found by the diff test
(`run.py`). Each entry gives the replay, the step, the field and both values, the cause and
the spec reference. These are reports: Razdor's rules are not changed here.

Run: `tools/difftest/rk1-day1.jsonl` (РК1, knight; the first day, then the ruins' garrison
fought by explicit actions), `python -m tools.difftest.run --actions
tools/difftest/rk1-day1.jsonl --name rk1-day1`, original's music held off, draw trace on.
"Step-local" is the run where each Razdor step starts from the original's generator state.

## 1. No draw for the idle patrollers when the hero stops

- Steps 2, 5, 7, 9, 12, 14, 16 (every stop of a walk or a wait). Step 5 (`wait 1`),
  step-local: `rng` Razdor 120733505, original 18883840, the original one draw ahead; its
  trace: `Random(3000)` from 0x4ad933 at the end of the wait.
- The original draws `Random(3000)` for every idle patroller each time the hero stops
  (0x4ad8a0, the armies' idle-animation offset); Razdor has no such draw, so its stream falls
  behind by one draw per idle patroller per stop and every later roll differs.
- Spec: engine.md §3.4, row 0x4ad8a0 ("each time the hero stops").

## 2. A village entered while an event fires: offers, chords and tribute come too early

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

- Step 18, the robber (type 66) of ruins 8: shot defence 9 in the original's battle record
  (+0x3c; base 5 plus the Round shield, item 51, `d-DefenceShot=4`), 5 in Razdor (building
  defence 3 on both sides). Step 21 (`battle_act 2,1,4`, the hunter's shot, attack 20):
  `battle.sides[1][0].hp` 57 in the original (20 − 12 = 8), 53 in Razdor (20 − 8 = 12).
- The original gives a ruins' first five goods to its garrison at load (worn or packed,
  0x4b554e, 0x4a273c); Razdor keeps them as the ruins' treasure only.
- Spec: economy.md §3 "Loot after the player's win", ruins ("their first 5 goods become
  garrison items, worn or packed").

## 5. An AI army one cell further on after a stop (open)

- Step 9 (`click_map 36,23`, the walk stopped by event 7), step-local: `armies[id 1].x`
  Razdor 6, original 7 (y 10, same game time); still so at steps 10 and 11, equal again at
  step 12. From step 14 on (step-local) the AI's goal refreshes come in another order and
  number (original: one `Random(40)` set, then two `Random(17)/Random(14)` sets; Razdor: one
  17/14 set, two 40 sets, one 17/14, one 40) and the armies drift apart; the midnight
  restock of market 5 (step 16) then starts from another state and stocks other goods.
- Not pinned down: the draws of steps 9-12 agree in number and values, so the cause is in
  how far the army gets by the stop. The original moves the AI by the smooth game time
  between ticks (0x4ae42f) and snaps unfinished steps when the hero stops (0x4ad8a0, §1);
  Razdor steps whole cells on its ticks. Needs a finer trace of army 1 (+0x1718 step state).
- Spec: world.md §2 (AI movement budget), engine.md §3.4 (0x4a2550 wander points).

## Not differences

- **Events done** and **event results while the window is up**: the original counts an event
  and applies its finishing results (an army switched off: step 9 `armies[id 14].active`)
  when its window is closed (events.md §6.1, §7.2); Razdor at once. Same order of events
  (7, then 6). The differ counts the shown event as done and reports such fields as window
  timing.
- **Unit level** (0 against 1): fixed in `razdor --replay` (the state numbers levels from 0,
  as the map file and the unit record do).
- **Music**: with the timed change held off, the only music draws seen are the game's own
  (`Random(8), Random(60000)` when the victory report closes, step 38).
