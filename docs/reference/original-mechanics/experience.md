# Discord Times experience: levels, battle XP, promotion

This file covers the experience system of the Community Update (Unstable) `DiscordTimes.exe`
(Delphi, image base 0x400000): unit strength, the XP a battle pays, levels and what they
add, promotion through the upgrade tree, and every other source of XP. The rules are in our
own words; addresses are virtual addresses in that build, given as evidence only.

Confidence tags:
- **code**: confirmed by reading the code.
- **data**: consistent with the data files, the texts or the footage, but not traced in code.
- **unknown**: not determined; Razdor keeps a documented guess.

## 0. Conventions

**Level numbers.** The exe stores a unit's level from **0**: a hired unit, the hero at the
start and a unit an event adds all get level 0 (AddUnit 495ce0 called with 0 from the
barracks 4b0fab, the hero 4b4383 and events 4a94d3). The interface shows it plus one: the
footage has a fresh unit at "Уровень 1, Опыт 0 / 580". Troop levels in a map (preset,
army, garrison triples, army byte 27) are stored 0-based too and copied as they are
(4b44e9, 4b52fb, 4b46cf). Razdor counts from 1, so **Razdor level = exe level + 1**; the
formulas below use `L` for the exe's 0-based level. **code**

**Rounding.** `Round` (402dd0) rounds halves to even; integer `div` truncates. **code**

**Records.** A unit in an army is 0x1DB bytes: +0 type (0-based), +4 XP towards the next
level, +8 the last gain (shown as "Опыт +N" after a battle), +0x10 level `L`, +0x20 HP
(−1 = unhurt), +0xDD current stats with items, +0x11D stats of the level without items,
+0x1AA and +0x1AE the two strength values of §1. In battle a unit is 0xA5 bytes (layout in
the battle notes); +0x64 is its strength, +0x68 its battle role, +0x6D, +0x71, +0x91 the
action counters of §3 and +0xA0 its XP share. **code**

## 1. Unit strength ("tactical cost")

The strength of a unit is computed from its **stats**, not from its price (49fc50, called
through 4a02a0). `Cost` plays no part. **code**

Inputs, from the unit's stat block: maximum HP `H`, melee attack/defence `AB`/`DB`, ranged
attack/defence `AS`/`DS`, magic power `MP`, school and direction, protections `PL`, `PD`,
`PE`, `Regen`, `Vamp`, initiative `I`, actions `M`, the bonus; and `bd`, the defence of the
building the unit's army stands in (army +0x378C, copied into every unit at 4a17c8).

1. If `H = 0` the strength is 0.
2. **Toughness.** A unit counts as melee unless `AS > AB and AS > MP` (a shooter) or
   `AB/2 < MP and AS/2 < MP` (a caster).
   - Melee: `x = DB + bd`, `y = H·Regen/100 + DS + bd`.
   - Others: `x = max(0, DB + bd − 5)` (0 when `DB + bd ≤ 5`), `y = H·Regen/100 + DS + bd + 5`.
   - `D = round(H · (e^(x/21.5)/1.17 + e^(y/30.3)/1.07))`.
   - Bonus: SpearDefense ×1.15; Unvulnerabe or Ghost `D = 10·H`; VampirsGist,
     OldVampirsGist or Evasive ×1.5; Garrison ×2.
   - `T₁ = H + D`; Dead or FastDead ×1.7.
   - `T₂ = T₁ · (1 + (PL + PD + 1.5·PE)/560)`.
3. **Attack value** `A`:
   - `A = AB` if `AB ≥ AS and AB ≥ MP`; then `A = 1.4·AS` if `AS ≥ AB and AS ≥ MP` (a tie
     goes to the bow).
   - ArmorIgnore, VampirsGist, OldVampirsGist or Artillery: `A += 18`.
   - If `M > 0 and MP > AB and MP > AS`: `A = MP`, then by direction: ToEnemy
     `A = 0.8·A·(M−1)/M + 0.2·A` (+25 for the Elemental school); ToAll `A ×= 1.2`; ToAlly
     unchanged.
   - Garrison `A ×= 2`; GodAnger `+10`; GodStrike `+20`; Counterblow `+AB`; FlankStrike
     `+ AB div 3`.
   - `A = M·A + E`, where `E = 0.15·A` for HorseAtack, OldVampirsGist and FastDead, else 0.
   - `A = A·(1 + I/100)` if `I > 0`, else `A = 0`.
4. `T₃ = T₂ · (1 + Vamp/100 · A/H)`.
5. `S = round(3.2 · T₃ · (A + 1) / 200)`; DeathCurse or Ghost `+150`; Poison `×1.1`
   (applied before the rounding).
6. If `AS ≥ ShotWeaponRange` (60 in `_Global.ini`) and the unit is not Artillery: `S = S div 3`.
7. `S = 1` when it came out 0.

The **tactical cost** is `S × CostMultipler div 100` (4a02a0), made at least 1 by a
Community hook (c25d86). The army keeps two: +0x1AA from the level stats without items and
`bd = 0` (used by AI hiring, §5), and +0x1AE from the current stats with items and the
building's defence (used in battle). The sum of +0x1AE over an army is its "army strength"
(army +0x1648, 4a182b), which the scenario conditions compare. **code**

Worked example (a synthetic warrior: `H` 50, `AB` 20, `DB` = `DS` = 5, `I` 10, `M` 1, no
bonus): `D = round(50·(1.0785 + 1.1023)) = 109`, `T = 159`, `A = 22`,
`S = round(3.2·159·23/200) = 59`.

Two consequences of the data: the Community archers reach `AS 60` after a few levels and
their strength then drops to a third; a warrior-priest whose magic outgrows its melee is
valued by its (weaker) spell term.

## 2. Levels

**XP needed** from level `L` to `L+1` (492680 with the series 492630):
`need(L) = round(StartExpirience · (LevelMultipler/100)^L)`, taken as the difference of two
partial sums of the geometric series. A fresh unit (L = 0) needs `StartExpirience`. In
Razdor's numbering: level `n` needs `round(Start · r^(n−1))`. **code**

Footage check: the fresh "0 / 580" unit is `StartExpirience=580` (unit 28); "level 2,
33 / 560" is unit 14 (`400 × 1.4`); the archmage hero "level 5, 500 / 590" is
`90 × 1.6⁴ = 589.8`. **data**

**Gaining XP** (49273c): a gain of 0 or less adds nothing. Otherwise the gain is stored as the
"last gain" (+8), added to the XP, and while the XP reaches `need(L)` the level rises and
`need` is subtracted. What is left is kept towards the next level. **code**

**Stats per level** (4908a8, recomputed from the type every time): **code**
- Hits, AttackBlow, DefenceBlow, AttackShot, DefenceShot, Initiative, Manevres:
  `base + d-stat × L` (4907a0).
- MagicPower: the same, but only for a type with a school; without one it is 0.
- ProtectLife, ProtectDeath, ProtectElemental, Regen, Vampirizm (4907c4): each level cuts what
  is left to 100 by `d` percent, `100 − round((100 − base)·(1 − d/100)^L)`, at most 99 (the
  cap applies even at level 0).
- Then the worn items, as in economy.md.

**Hit points.** A level-up changes only XP and level. HP is kept as it is: an unhurt unit
(HP −1) stands at the new maximum, a wounded one keeps its number. **code**

## 3. Battle XP

The end of a battle (48bb10) recomputes both sides' strength (483ecc), then pays each side's
survivors a share of a pool. **code**

**Side strength** (483ecc; before the fight, from 48b75c, and again at the end):
- Each unit counts `v = round(strength × HP / maxHP)`, by row: front `R₁`, back `R₂`, reserve
  `R₃`.
- A back-row unit whose **battle role** is not melee counts twice. The role (4836cc): score =
  the largest of `AB`, `AS`, `MP` plus a third (integer) of the other two, +10 GodAnger, +15
  ArmorIgnore, +20 GodStrike, +AB Counterblow, +10 FlankStrike; a shooter if
  `AS ≥ score/1.5`, a mage if `MP ≥ score/1.5` (mage wins), otherwise melee.
- `f = min(1, 0.8·R₁/R₂ + 0.2)` when `R₂ ≠ 0`, else 1; strength = `round(R₂·f + R₃ + R₁)`.
- A side reduced to one unit whose role is not melee counts a fifth (`div 5`).

**Per side** (both sides, in the order player, enemy):
- `E₀` = the enemy's strength at the start, `HP₀` = the sum of the side's HP at the start
  (48b8e7), `lost` = HP the side lost in the battle (every wound, capped at the unit's HP,
  48a354; a unit killed by a DeathCurse or Ghost adds its remaining HP, 48a45c). Healing does
  not reduce `lost`.
- The exe computes `ratio = E₀ / own₀`, `k = 1 ± |ratio−1|·ExpCorrection/100` clamped to
  [0.25, 4], and `MainExpCorrection × k × (E₀ − E_end)/100` (48bc59), and stores it
  (side +0x7F0) — but **nothing reads it**. `MainExpCorrection` and `ExpCorrection` do not
  change the XP. The pool's "damage exchange" term reads two side fields (+8, +0x14) that no
  code ever writes, so it is always 0. **code**
- **Pool** = `round((E₀ div 20) × max(0, (HP₀ − lost)/HP₀))`, or `E₀ div 20` when nothing
  was lost.
- `t = pool × 0.25 / N₀`, where `N₀` is the side's unit count **at the start** (the dead
  count in the divisor).
- For each **surviving** unit (the dead records are gone by then):
  `share = t·((4 − row) + row · useful/(taken + left))`, row 1 front, 2 back, 3 reserve,
  where `useful` counts its attacks and spells (hostile and friendly), `taken` all its
  actions (moves and passes too), `left` the actions it still had in the last turn; with
  `taken + left = 0` the share is just `t`. A share below 0.5 becomes 1; the award is
  `round(share)`. So an idle front unit gets `3t`, an idle reserve unit `t`, and any unit
  that spent all its actions attacking `4t`. **code**
- The hero is an ordinary unit here: he shares the pool by the same rule. **code**
- A side that surrenders (all its units have `Surrender > 0`) is paid its XP first; then its
  units die and their `Surrender` values are summed (side +0x7F8) for the victor's mana.
  **code**

**The player's units** (victory only; 4c5657 with the Community hook c2518f): each survivor
gains `round(award × HeroExpirienceModificator × F × C / 1 000 000)`, where `F` is the
difficulty factor (120, or 100 with "impossible difficulty", `OptValue10`) and `C` the beaten
army's experience correction (map byte 71). A captured building's garrison record is reset
with `C = 100` just before (4c55b9), so a garrison always counts 100. The gain is made
non-negative and capped at **5256** (c25264), then added by 49273c. The cap is per unit and
battle; XP left over after a level is kept. The code under the Community hook does the same
product in integers (truncating). A defeat or a stalemate pays nothing: the XP code is only on the victory
path (4c50ec; the other result screen 4c57bc has none). **code**

`HeroExpirienceModificator` applies to **every** unit of the player, not just the hero.
With the shipped values (50, F 100) a share of 48 shows as "Опыт +24". **code** / **data**

**The video's rate (2026-09-29).** The gameplay video's fort battle ("Форт в Трясине", РК3,
09:49) checks the formula end to end: the garrison's starting strength is 1516, the pool 75,
and a unit that attacked with every action gets a share of 25; the cuirassier, the sorceress
and the hero show "+25", "+24", "+26". So that game paid shares at ×1.0
(`HeroExpirienceModificator × F × C / 10⁶ = 1`, e.g. modificator 100 with "impossible
difficulty"), while the Community Update's `_Global.ini` has 50 and pays half. **Razdor
plays with 100 whatever the install says** (`content::PLAYER_XP_MODIFICATOR`, the player's
choice); the rest of the formula is unchanged. Test: `real_fort_battle_pays_the_videos_xp`.
**data**

**AI armies** gain XP only in battles between AI armies (4a4c68): each side whose strength is
still above 0 at the end gets `award × AIExpiriencePercent div 100` for each survivor
(4a4a7c), with no difficulty factor, no army correction and no cap. An AI army that beats the
player gains nothing. AI units keep XP and levels like the player's (49273c), and after the
gain may take the upgrade tree (§4). **code**

## 4. Promotion

**The player** (the army screen, 4c3d38 → 4b1a04):
- Any unit **except the hero** (the first unit) can be promoted once its level `L ≥ 1`
  (Razdor level 2). `NextUnitNLevel` is **not** checked for the player (4c3d63). The hint
  texts follow the same test (4c39d8): "not enough experience" at `L = 0`, "choose an option"
  otherwise, "final class" without options. **code**
- Promotion is free. The unit becomes the chosen type with `L = 0`, XP 0 and last gain 0
  (4b1f67). HP is not touched. **code**
- What happens to worn items the new class may not wear: **unknown** (Razdor moves them to
  the pack).

**The AI** (4a4a7c, after every gain):
- Options are NextUnit1..3. The loader moves a lone option into slot 2 (4e080b).
- Militia (type 4) picks slot 1 with chance 1/3, else slot 3; Infantry (type 8) slot 3 with
  chance 1/3, else slot 1; every other class a random filled slot. **code**
- The pick is taken when its `NextUnitNLevel ≤ L`: new type, `L = 0`, XP 0, and its worn
  items are dropped. **code**
- In the Community data Militia has options only in slots 1 and 2 (and Infantry in 1 and
  2), so their special picks can land on an empty slot 3. What the exe then does (it would
  read a zero type) is **unknown**; Razdor promotes nobody that time.

## 5. Other XP sources

- **Events** (4ab51b): the event's XP goes to the **hero only**, as it is — no modifier, no
  cap; a negative value adds nothing. **code**
- **Community opcode 13** (c27f30): `m` XP to unit `g` of holder `x`, or to every record of
  it (`g = −1`), for the player, an AI army or a building's garrison, through the same
  49273c; AI units bank it like the player's. No promotion check. **code**
- **The level condition** of events compares the hero's 0-based level (4a80cc): "level 2"
  means Razdor's level 3. **code**
- **AI hiring** (4a67ac–4a6bac, when an AI army hires in a building): with map byte 14 ("add
  experience like the player") the army first computes
  `P = Σ(player's units: level strength +0x1AA + XP) div (player's unit count + 2)`. For every
  unit it hires: `X = P − (its level strength div 2)` when `P > 0`, else 0; plus map byte 19
  (bonus XP for hired units). If `X > 0` the unit gets `random(X) + X div 2` (`random(X)` is
  0..X−1), level by level, with a promotion try at every level (4a4c04). The player's hires
  get no XP. **code**
- **Hero preset.** The preset has **no starting experience**: offset 8 is the starting
  **gold** (read as a signed 16-bit value into the gold, 4b5831 → 4ab150) and offset 12 the
  starting **mana** (4b5842 → 0x68E4F4). The shipped maps agree: offset 8 is the same for
  all three classes, offset 12 is largest for the archmage. The hero starts at level 0 with
  0 XP. **code** / **data**
- **Campaign carry-over** (4b5b64): the hero's unit record always carries over; with header
  byte [3] off his XP and level are set to 0 (4b5d6b). With byte [6] the army carries over
  with its levels and XP. The rest is in economy.md. **code**
- **Surrender** gives mana, not XP (§3).

## 6. The hero

The hero levels exactly like a unit, with his class's `StartExpirience` and
`LevelMultipler` (100/150, 90/160, 80/170) and `d-*` gains; he shares the battle pool like
any survivor. He cannot be promoted. His level feeds nothing else that we found: the only
reader of the hero's level field is the event level condition (4a80cc). **code**

## Unknowns

- Worn items the player's promoted unit can no longer wear.
- The Militia and Infantry AI picks that land on an empty slot.
- Whether dead units count in the event "army strength" (the sum is over the army record).
- The pool when a side had no HP at the start (a division by zero in the exe; it does not
  happen in play).

## Razdor now → original

After this investigation Razdor follows the rules above; the rows show what changed and the
few places that still differ.

| Topic | Razdor before | Original (this exe) | Razdor now |
|---|---|---|---|
| Level numbering | 1 as hired | 0 as hired, shown +1 | 1 as hired (= exe + 1); the event level condition uses `level − 1` |
| XP needed | `Start·r^(level−1)` | the same | the same (partial sums, half-even) |
| Tactical cost | `Cost × CostMultipler/100`, +10%/level | from stats (§1) × CostMultipler | as the exe, cached per type and level |
| Side strength | sum of tactical costs | rows, back-row doubling, `f`, lone ÷5, HP-scaled | as the exe |
| Pool | `MainExpCorrection·k·destroyed + E/20` | `E₀ div 20 × HP kept` | as the exe |
| Share | weight `(4−row)·10 + damage + healing` | `t·((4−row) + row·useful/(taken+left))` | as the exe |
| Player modifier | `HeroExpirienceModificator` | × F × the beaten army's correction, cap 5256 | as the exe (F from `OptValue10`) |
| XP on a stalemate | paid | none | none |
| AI XP | the player's pool ×AI%/Hero% × correction | AI-vs-AI only, ×AI% | as the exe; AI units bank XP and promote |
| Cap | on every gain | per battle only | per battle only |
| Level-up HP | HP gain also heals | unhurt stays full, wounded keeps HP | as the exe |
| Percent stats | `+d` per level | gap to 100 closes by `d`% per level, max 99 | as the exe |
| Promotion | from `NextUnitNLevel`, level 1, HP fraction | any unit with a level, not the hero; level 0, XP 0, HP kept | as the exe; unwearable items to the pack *(guess)* |
| Event XP | the hero | the hero, as it is | the same; a new level shows on the map |
| Opcode 13 on AI troops | levels only | XP banked | XP banked |
| AI hires | no XP | bytes 14 and 19 (§5) | as the exe |
| Preset offset 8 / 12 | XP / gold | gold / mana | gold / mana; no starting XP |
| Carry-over [3] | level and XP | level and XP, else level 0 | `Game::apply_carry_over` |
