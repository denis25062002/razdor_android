# Differential testing against the original: design

## Goal

A loop that keeps comparing Razdor with the original `DiscordTimes.exe` on the same
situations, finds where they differ, and fixes Razdor until they agree:

1. generate a test case (armies, items, spells, options);
2. run it in the original and in Razdor, both without a window and without a person;
3. record both runs as the same numeric trace and find the first difference;
4. shrink the case to the smallest one that still differs;
5. let an agent fix Razdor's rules, with the case kept as a regression test;
6. accept the fix only if the case now matches and nothing that matched before breaks.

Success criteria:
- `razdor-difftest battle --cases 1000` runs unattended on Linux, compares 1000 random battles
  and prints how many match, with a report for each one that does not.
- A divergence report names the case, the first action that differs, the field, both values
  and the exe function that decided it (from the table in `battle.md` §0).
- Every divergence that gets fixed stays in the local regression corpus and runs on every
  loop, so an agreement once reached cannot be lost silently.
- The agent's fixes arrive as pull requests on `indicozy/razdor`; nothing merges without
  review.

Battles come first: the original's battles have **no randomness** (`battle.md` §4, "No
randomness"), so both programs must agree to the last hit point, and any difference is a
bug on one side. Experience, economy and AI come later on the same machinery (stage 5).

## Legal and content boundary

The rules of `2026-09-25-dt-revival-design.md` apply unchanged:
- The original runs from a **disposable copy** of the player's install, never from the install
  itself; nothing is written into `RAZDOR_DT_DIR`.
- Traces, cases and reports contain **numbers only** (unit type ids, stats, hit points, cells),
  like `src/rules/replay.rs` today; no texts, names or art of the original.
- The corpus, traces and reports live in the user's data folder
  (`~/.local/share/razdor/difftest/`), not in the repo. The repo holds only the tools, the
  schema and the oracle's source. A regression test in the repo refers to a case by its id
  and is skipped without `RAZDOR_DT_DIR` and the local corpus.
- The oracle DLL contains our own code only; it reads the game's memory at the addresses the
  reference docs already describe, and copies no code out of the exe.

## How the original runs: nobody plays it

The original is never played by hand, and no screen, clicks or keys are involved for
battles. It runs like this:

- **Wine, invisible.** `DiscordTimes.exe` runs under Wine on a virtual display (Xvfb), the
  same way Razdor's offscreen snapshots run today. Wine is the only new system package.
- **An injected DLL, `razdor-oracle.dll`.** The game loads DLLs from its own folder (it
  already loads `ddraw.dll` from DDrawCompat and the Community Update's `detoured.dll`), so in
  the disposable copy a proxy DLL is loaded at start-up. It is written in Rust and built with
  the llvm-mingw toolchain `scripts/dist.sh` already uses. `DiscordTimes.exe` is a 32-bit
  program (PE32, i386), so the DLL is 32-bit too (`i686-pc-windows-gnullvm`, llvm-mingw's
  `i686-w64-mingw32-clang`), and Wine must run 32-bit programs.
- **The game's own simulator.** Battle setup (48b75c) plays a complete AI-vs-AI copy of every
  battle before showing it ("Pre-simulation"), and AI armies fight each other off-screen
  with the same code (B+5 = 0). So the exe already contains a battle simulator that needs no
  screen. Once the game has loaded its data, the oracle builds the two armies of a case in
  memory (the 0x1DB army records of `experience.md` §0), calls that simulation, and hooks the
  functions of `battle.md` §0 to record each step:
  - 4840ec start of a turn, 489ca0 the next actor, 4864e0 the AI's choice, 48a5c4 one action,
    48a354 damage applied, 489f50 a unit removed, 48a170 row collapse, 48bb10 the end;
  - at each of them it writes the 0xA5-byte battle records of both sides into the trace.
- **Batches.** One start of the game runs a whole batch of cases (hundreds), because the game's
  start-up is the slow part. The oracle exits the process when the batch is done; a watchdog
  kills it after a timeout and marks the running case as "hung".

Who plays the player's side: nobody. Both sides are played by the original's battle AI, just
as in its AI-vs-AI battles. Razdor does the same with the battle AI on both sides
(`Battle::auto_play_to_end`, the quick battle). This also compares the two battle AIs,
which is intended: the AI is part of the rules.

The interactive battle differs from the simulation in a few documented ways (Splash works
only on screen, flag 4ed424; the player side's auto actions use the "killable" test of
B+5 = 2). The first cases run in simulation mode on both sides; a later case kind sets the
interactive flags, once stage 3 works.

## Architecture

```
oracle/                      new crate: razdor-oracle.dll (cdylib, i686-pc-windows-gnullvm)
  src/lib.rs                 DllMain, loads the batch, calls the simulation, exits
  src/hooks.rs               inline hooks at the battle functions (a small trampoline, no C)
  src/records.rs             reads the 0xA5 / 0x1DB records into the trace schema
src/difftest/                new module of the razdor library, pure, no macroquad
  case.rs                    Case: both armies (unit type, level, items, HP), options, seed
  trace.rs                   Trace: the schema below; Razdor's side is written by the battle
  generate.rs                random cases from the install's units, items and spells
  compare.rs                 first divergence, with the exe function behind it
  shrink.rs                  delta debugging of a failing case
src/bin/razdor-difftest.rs   the runner: generate, run both, compare, shrink, report
scripts/difftest-loop.sh     the whole loop, including the fix agent (stage 4)
```

The trace hooks on Razdor's side go into `rules::battle` behind a `Tracer` that does nothing
unless a test sets it, so the game pays nothing for them.

### Trace schema

One JSON line per event, the same on both sides:

```json
{"ev":"turn","round":3}
{"ev":"actor","side":1,"pos":4,"threshold":9}
{"ev":"action","side":1,"pos":4,"kind":4,"target":[2,7]}
{"ev":"damage","side":2,"pos":7,"amount":17,"hp":33}
{"ev":"removed","side":2,"pos":7}
{"ev":"collapse","side":2,"row":1}
{"ev":"state","side":1,"units":[{"pos":0,"type":12,"hp":70,"ab":45,"db":15,"ds":10,"ini":9,"mnv":1,"power":0}]}
{"ev":"end","winner":1,"rounds":7,"xp":[31,31,0]}
```

- Units are named by side and position in the side's record list, as the exe does, never by
  name. `kind` uses the exe's action kinds (4 melee, 5 long strike, 7 shot, 0xB–0xE magic).
- `state` lines come at every turn start, so a difference is found at most one turn after it
  happens, even in a field no action line shows.
- Razdor maps its own ids to the exe's (Razdor levels are the exe's + 1, `experience.md` §0;
  unit and item ids are the ini order). The mapping lives in `trace.rs`, in one place.

### Cases

A case is small and self-contained: the two armies (up to 12 units each: type id, level,
the four items, HP, row and column), the options that change battles (`OptValue9`,
`OptValue11` for 6 columns, difficulty), the hero class of each side (the Knight's damage
rule) and the building defence. The generator draws them from the install's own units,
items and spells, and biases towards what is least covered: unit bonuses no case has used
yet, items with rare modifiers, the 4- and 6-column layouts, reserve rows, casters.

A case that differs is shrunk before anyone looks at it: remove units, items, levels and
options one at a time (delta debugging) while the first divergence stays the same. The
result is usually two or three units, which makes the cause readable.

### Divergence report

```
case 7f3a91 (shrunk from 12+11 units to 2+1)
first divergence: round 2, action of side 1 pos 0 (unit type 14, Bonus SpearDefense)
  field: damage to side 2 pos 0
  original: 12      razdor: 17
  decided in: 485908 physical damage (battle.md §0, "Physical damage formula")
  razdor: rules::battle::physical_damage
trace files: ~/.local/share/razdor/difftest/cases/7f3a91/{original,razdor}.jsonl
```

## The fix agent (stage 4)

`scripts/difftest-loop.sh` runs the loop on a schedule (nightly, or the Claude Code
`schedule` routines):

1. Run a batch; collect the shrunk divergences, grouped by the exe function that decided
   them.
2. For the largest group, start Claude Code headless (`claude -p`) in a fresh git worktree
   with the report, the two traces and the section of `docs/reference/original-mechanics/`
   that covers the function. Its task: explain the difference, fix `src/rules`, and add a
   unit test that reproduces the case with our own numbers (no original data in the repo),
   plus the case id in the local corpus.
3. **Gate**, all automatic: `cargo test` passes; the case now matches; the whole local
   corpus still matches where it matched before (no regressions); the fix touches only
   `src/rules`, its tests and the reference docs.
4. If the gate passes, the agent opens a pull request with the report and its explanation. If
   it fails three times, the group is marked "needs a human" with the agent's notes.

When the original's behaviour is unclear from the docs, the agent may read the exe's code at
the named function (disassembly with Ghidra headless, local only) and update the reference
doc in our own words, with the confidence tag, as the docs do today. It never changes
Razdor to match a trace it cannot explain: an unexplained difference stays open, because
the oracle itself can be wrong.

**Checking the oracle.** The oracle is code too. Before its traces are trusted, stage 2 runs
battles that the reference docs already work out by hand (the damage formula examples in
`battle.md`), and a sample of oracle runs is compared with the same battle played on screen
in the original, looked at by a person once.

## Stages

Each stage ends with something that works on its own.

1. **The original runs headless.** Wine installed (needs sudo once); the disposable copy of
   the install; `DiscordTimes.exe` starts under Xvfb and reaches its main menu; a screenshot
   proves it. *Exit:* one command starts and stops the original with no window visible.
   **Done (2026-09-29):** `scripts/run-original.sh <out-dir> <sec>...` (Wine 11.0 on Fedora 44)
   reaches the main menu in about 15 seconds and draws at about 400 FPS. What it took:
   - a **Russian locale** (`LANG=ru_RU.UTF-8`): Wine gives programs the ANSI code page of the
     locale, and with a Western one the Cyrillic map names cannot be read, so the game
     crashes while loading (page fault reading 0000012B at 00402BFD);
   - a **sound driver**: with none the game stops at "No sound driver is available for use";
     Wine's ALSA driver gets a private config whose default device is ALSA's `null`, so the
     game has a driver and nothing is heard;
   - DDrawCompat (the game's own `ddraw.dll`) works under Wine as shipped.
2. **The oracle records one battle.** The proxy DLL loads, waits for the data, builds one
   hard-coded 1-against-1 case, runs the simulation and writes its trace. Its numbers agree
   with a hand calculation from `battle.md`. *Exit:* a trace of a known battle, checked by
   hand. This is the riskiest stage: calling the simulation outside a real battle may need
   more state set up than the docs say (see Risks).
3. **Razdor traces and the comparator.** `Tracer` in `rules::battle`, the schema, the case
   format, `compare.rs`, the runner. *Exit:* `razdor-difftest battle --cases 100` prints a
   match rate and reports.
4. **Generator, shrinker, corpus.** Random cases with the coverage bias, delta debugging,
   the local corpus with its matched/unmatched history. *Exit:* a nightly run of 1000 cases
   and a list of shrunk divergences grouped by function.
5. **The fix agent.** The script, the worktree, the gate, the pull requests. *Exit:* one
   divergence fixed end to end by the loop, reviewed and merged.
6. **Beyond battles**, on the same trace machinery: battle XP (48bb10, `experience.md`),
   prices and wages (`economy.md`), then the world AI (`world.md`). These use the game's
   random generator (4832fc), so the oracle sets its seed per case and Razdor's
   `rules::rng` has to reproduce the exe's generator first.

A screen comparison (screenshots of the original's windows against `RAZDOR_SCENE`
snapshots) is useful for the interface but separate from this loop; it can reuse the
stage 1 set-up.

## Risks and unknowns

- **Calling the simulation directly.** The pre-simulation runs inside battle setup, with the
  two armies already in place. Building armies in memory and calling it may need more global
  state (the options block, the battle globals at 4ed0xx–4ed4xx). Fallback: generate a
  tiny `.DTm` map with Razdor's editor (the original loads its maps) that puts an enemy
  army next to the hero, and let the oracle start that battle through the normal path with
  the player's side on auto.
- **Wine.** The rules run the same under Wine as on Windows; timing, sound and DirectDraw may
  not. The game runs with sound off and DDrawCompat as shipped. If Wine fails, a Windows VM
  runs the same oracle unchanged.
- **The Community Update's own hooks.** Community bonuses are hooks in the exe (addresses
  c2xxxx in `battle.md` §7). The oracle's hooks must not sit on the same bytes; it hooks the
  function entries of §0, which the Community code calls into.
- **Speed.** A battle in the original's simulation should take milliseconds; start-up takes
  seconds. Batches keep a run of 1000 cases in minutes. To be measured in stage 2.
- **Agent fixes that fit the trace but not the rule.** Guarded by the gate (the whole corpus
  must keep matching), by the requirement to explain each fix from the exe's behaviour, and
  by review of every pull request.

## Out of scope

- Playing the original through its interface (clicks, keys, screen reading) for battles.
- Changing the original game or the player's install.
- Merging without review; publishing traces or cases.
- Rules the original does not have (the parity rule: Razdor matches the original first).
