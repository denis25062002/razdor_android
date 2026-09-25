# Discord Times battle mechanics: reverse-engineering notes

This file covers the Community Update (Unstable) `DiscordTimes.exe` (Delphi, image base 0x400000).
The rules below are in our own words. Addresses are virtual addresses in that build, given as
evidence only. Working notes (layouts, options, AI, bonuses) stayed with the investigation and are not part of the repo.

Confidence tags:
- **code**: confirmed by reading the code.
- **data**: consistent with the data files or help text, but not traced in code.
- **unknown**: not determined.

## 0. Battle model

**Sides.** Side 1 is always the player's army and side 2 the enemy. Both battle starts
(4d2338 and 4d2358) pass the player's army as side 1, and the end-of-battle UI (4c50ec) reads
side 1 as the player. **code**

**Records.** Each side has a 3×6 grid of unit indices: row 1 is the front, row 2 the back,
row 3 the reserve. It also has up to 12 unit records of 0xA5 bytes. The record layout is in
notes/layout.md.

**Main functions:**

| Address | Role |
|---|---|
| 48b75c | Battle setup |
| 4840ec | Start of a turn |
| 489ca0 | Picks the next actor |
| 484c4c | Legal-cell map |
| 4864e0 | AI choice |
| 48a5c4 | Performs one action |
| 485908 | Physical damage |
| 485b3c | Magic power by action |
| 48a354 | Applies damage |
| 48a3f0 | On-kill effects |
| 489f50 | Removes a dead unit |
| 48a170 | Row collapse |
| 48bb10 | End of battle (XP and surrender) |
| 4c50ec | Victory or defeat screen and loot |

**Damage kinds.** 4 = melee, 5 = long (flank) strike, 7 = shot. Magic kinds are 0xB bless,
0xC heal, 0xD curse and 0xE strike.

### Physical damage formula (485908; the base code, Community hooks in section 7)

- `atk = Attack(kind) + atkModifier`. Attack is AttackBlow for kinds 4 and 5, AttackShot for kind 7.
- `def = Defence(kind) + defModifier`, clamped to at least 0.
- **Melee only:**
  - SpearDefense on battle turn 1: `def ×= 3`.
  - Piercing: ArmorIgnore, PoisonArmorIgnore, VampirsGist or OldVampirsGist sets `def = 0`.
  - Long strike: `def /= 2`, and FlankStrike gives `atk ×= 2`.
  - Then `def += buildingDefence`.
- **Shot only:**
  - Piercing: ArmorIgnore, PoisonArmorIgnore or **Artillery** sets `def = 0`. The vampire gifts
    do **not** pierce against shots.
  - Then `def += Row2Def` if the target is in row 2, and `def += buildingDefence`.
- `dmg = atk − def` if `atk > def`, else 1. This is the subtractive mode, B+1 = 1, which the
  real battle always uses. The percentage mode `atk·(1−def/100)` is unused.
- The target modifiers follow in this order:
  1. Evasive, VampirsGist or OldVampirsGist: ×2/3 (integer). Community Assault can also apply
     ×2/3 here (c2a403, section 7).
  2. Garrison with building defence ≥ 10: ×2/3.
  3. Dead or FastDead against a shot: ×3/10.
  4. Knight army: `× Knight% / 100`.
  5. Unvulnerabe or Ghost: `dmg = 1`.
  6. The attacker's GodAnger adds +10 and GodStrike +20. If the result is 0 it becomes 1.
  7. Community hook (Evasion and others).
- **Knight%.** The value is loaded at 4e4501. This build **sets it to 80 unconditionally** when
  `[GlobalOptions]` is read: the army takes −20% physical damage. The data default is 90 and the
  help text says 10%. **code**

**Magic power by action (485b3c).** Hostile kinds 0xD and 0xE use
`P = round(MP × (1 − Protect_school/100))`. The nature multipliers and the heal and bless
exclusions are exactly as in mechanics.md §3.3. **code**

## 1. Buff and curse duration, stacking, magic drain

**Duration.** **code**
- Every blessing and curse is stored in three per-unit modifier fields: attack, defence and
  initiative.
- The start of **every** battle turn (4840ec, 484238–484288) sets all three to 0 and clears the
  blessed and cursed flags. So a blessing or curse lasts **for the rest of the turn in which it
  is cast**, and it is gone when the next turn starts.
- There is no duration counter. In practice a caster with high initiative gets the most out of
  its spells.
- Elemental bless and curse change the target's **actions left** directly. That only affects the
  current turn. Curses cannot push it below 0.

**Stacking.** **code**
- Values are added (+=) to the modifier fields, not replaced.
- A friendly mage cannot pick an already-blessed unit in the same turn, unless the unit is
  wounded, and then it heals instead.
- A hostile mage whose target already has a negative modifier strikes instead of cursing
  (section 3).
- So in practice there is at most one blessing and one curse per unit per turn. The flags reset
  every turn.

**EternalGift** (hooks c29e59, c29ea3, c29f2f for curses; c2a019, c2a0ae, c2a0f8 for blessings).
**code**
- The caster's blessings and curses write to the target's **base** stats instead of the
  modifiers. So they last the whole battle **and stack** with every cast.
- Elemental: base initiative changes by the amount. The change to actions left still applies to
  the current turn only.
- Life and Death: attack and both defences change.
- Bug: an EternalGift **Life blessing** *lowers* both defences by the defence amount, while it
  raises attack. The attack goes to AttackBlow, or to AttackShot when AttackBlow is 0.

**Magic power drain.** Community replaces the whole vanilla block (c2851a). **code**
- From turn 2, for each unit with MP > 0, at the start of the turn:
  1. `MP −= ManaDrain`;
  2. `MP = max(MP, MinMagicPower)`;
  3. `MP = max(MP, 0)`.
- **Defaults** by school (an ini value of 0 falls back to the default):
  - `ManaDrain`: DecSpellLife 2, DecSpellDeath 2, DecSpellElemental 5.
  - `MinMagicPower`: MinSpellLife 15, MinSpellDeath 0, MinSpellElemental 15. **Undead Death
    casters get +25** on the floor.
- The floor also **raises** a weaker caster. A Life mage with MP 10 has 15 on turn 2.
- **Concentration** *adds* the drain instead of subtracting it, with no cap.
- The replaced vanilla code had the same idea: Life floor 15; Elemental floor 15 (0 for Undead or
  Rogue casters that went negative); Undead Death casters 25.

## 2. Reserve row, movement and collapse

**Movement options (484c4c, helpers 4848a4 and 484b64).** **code**

- **Every action costs one action,** moves and "pass" included. 48a5c4 decrements actions-left
  before it does anything else.
- **From row 1 or row 2:** a unit may move to an empty cell of row 1 or row 2 in columns
  c−1, c or c+1. It may also move to **any** empty reserve cell, but only while its per-turn
  reserve flag is set. This works from the **front row as well as the back row.**
- **From the reserve:** a unit may move to any empty cell of row 1 or row 2, in any column, but
  only while the flag is set.
- **The reserve flag.**
  - It is set to 1 for every unit at the start of each turn (4840ec).
  - It is cleared when a unit moves into or out of the reserve (48a7b7–48a813).
  - Result: one reserve transition per unit per turn. A unit that entered the reserve cannot
    leave it that turn, and a unit that left it cannot go back that turn.
- **No swapping.** A unit can never swap places with an own unit. Only empty cells are offered.
- **Clicking one's own cell** (code 1) passes one action. It increments a counter that the XP
  code reads.
- **Map code 3** would pull an enemy back-row unit into its front row. It exists only when the
  constant at 4ed38c is 0, and in this build it is 1, so the code is dead.

**Collapse (489f50 → 48a170).** **code**

The check runs right after any unit of a side is removed as dead. It also runs for the actor's
own side when the actor has used its last action (48b5ac).
- **Rows 1 and 2 both empty:** every reserve unit moves to **row 1**, in the same column, and its
  remaining actions for this turn are set to 0. The reserve never steps into row 2.
- **Only row 1 empty:** every row-2 unit moves to row 1, in the same column, and keeps its actions.
- **A voluntary move** that empties the front row therefore makes the back row step forward only
  once that unit has finished its actions.
- **Removal of the dead:** a dead unit is taken out of the side's list. Later records shift down
  and the side's unit count drops (hook c252bd).

**Row 2 defence.** Row2Def (+5) is added only against shots, after any piercing (485a04). So a
piercing shooter still faces Row2Def and building defence. **code**

## 3. Mage action choice and targets

**Hostile mages.** There is **no player choice** between strike and curse. The player's click
(4c43e8) and the AI both execute the cell's code 8, and 48a5c4 (48ac99–48acf1) decides:
- if the target already has any negative attack, defence or initiative modifier this turn, the
  mage **strikes**;
- otherwise it **curses**.

Since modifiers are wiped at every turn start (section 1), each fresh target is cursed first and
struck afterwards. This holds for every school, Life included. **code**

The docs' "strike for Life" rule came from the hover preview (4c3e58). It does not decide the
action, and it was not re-checked here.

**Friendly mages.** The action on an ally (code 9) is a **heal** when the target is wounded and the
heal power is above 0; otherwise it is a **bless** (48a87c–48a937). **code**

**Legal targets.** **code**
- **Hostile mage (ToAll or ToEnemy):**
  - From row 2: every enemy in rows 1–2.
  - From row 1: the same, but only if the three opposite enemy front cells are empty; otherwise
    it cannot cast at all.
  - Ghost-bonus casters may also cast at the three opposite front cells from any row, the
    reserve included (48555b).
- **Friendly mage (ToAll or ToAlly), from rows 1–2:**
  - It may target any own unit in rows 1–2, in any column, that is wounded **or** not yet
    blessed this turn (flag +0x9d).
  - An Elemental caster cannot target an Elemental-school ally at full HP.
  - Units crippled by NoHeal are never offered (Community hook c2967a).
- **Caster in the reserve:** it may target every own reserve unit, and nothing else (4857fb).

**Stacking.**
- **Blessings:** their values are added to the modifiers, but an ally cannot be blessed twice in
  one turn because blessed units are no longer legal targets. A wounded, blessed unit can still
  be healed.
- **Curses:** in practice one per target per turn. After the first curse the auto-choice
  switches to strikes.

**Extra: Undead casters drain.** If the **caster** is Undead and its school is Elemental or Death,
its curse also drains `(P / CurseMainSpell) / 2 + 1` HP. The target loses this amount and the
caster gains it, capped at max HP (48afce and 48b100). Life-school curses do not drain. **code**

**Extra: vampirism on magic.** A Death-school **strike** heals the caster by
`Vampirizm% × strike damage`, but not against Undead or Elemental targets (48ad61). **code**

**Curse and bless sizes.** The base code agrees with §3.3, with one detail.
- The Life curse uses the integer divisor `floor(2·CurseMainSpell/3)` (= 3), stored at 4ed3a8 by
  48b7b8, and the constant 10 at 4ed3b0 (not the ini's CurseNextSpell). So the defence loss is
  `P/3 + 1` and the attack loss `P/10`.
- An Elemental curse cannot bring actions-left below 0.

## 4. AI in battle

Full detail is in notes/ai.md. Everything here is **code** unless marked.

**Framework.**
- One routine, 4864e0, serves both sides. The picker 4860cc scores cells with a strict ">",
  scanning columns in the preferred order: 3,2,4,1, or 4,3,5,2,6,1 with six columns. The first
  cell in that order wins ties, and a score of 0 is never chosen.
- **Unit strength** (4836cc):
  - `power = max(AB, AS, MP) + (sum of the other two)/3`.
  - Bonuses add to it: GodAnger +10, ArmorIgnore +15, GodStrike +20, Counterblow +AB,
    FlankStrike +10.
  - Role: shooter if `AS ≥ power/1.5`, mage if `MP ≥ power/1.5` (mage wins a tie), else warrior.

**Priority order.** The first step that yields a positive score wins.
1. **Front-row retreat.** A non-warrior in row 1 with more than 1 action left moves back when
   another own unit is in the front row. It picks the back cell behind the front unit with the
   most HP. The score is 1000 + that unit's HP.
2. **Melee** (row-1 targets):
   - `score = dmg × round((R + 1) × targetManevres)`, where R is the target's return damage on
     the actor.
   - Killable target: `round((R + 1) × M) × 100` instead.
   - An attacker with Poison against an unpoisoned target: ×2.
3. **Shots:**
   - `score = round((targetPower + 1) × dmg × (M + √targetActionsLeft))`.
   - Killable ×4.
   - Back-row targets: a warrior ÷3, a shooter ×1.5, a mage ×7/4.
   - A target with 1 Manevres: ÷2.
4. **Magic by school.** notes/ai.md has the formulas.
   - Life heals the biggest wound, but only one of at least MP/4, and not on Undead. Otherwise
     it blesses low-defence, strong front-liners. Its strikes prefer Undead ×3.
   - Elemental compares haste or slow against heal or strike, per side.
   - Death weighs urgency against threat by role; a nearly dead Death caster may target itself.
   - Scoring is **medium** confidence here, and the Elemental details are approximate.
5. **Moves, only when nothing else scored:**
   - Back-row pure warriors step forward, preferring a column facing an enemy.
   - Front-row units shift sideways toward the most enemies.
   - Reserve units come out: warriors to row 1 or 2, others to the nearest back-row cell. A mage
     in the reserve heals wounded reserve units instead.
   - The AI **never** moves a unit into the reserve.

**"Killable" and the difficulty option.**
- Normally the enemy AI counts a target as killable only if one hit kills it:
  `HP ≤ dmg` (486ca3, 486f71).
- With OptValue9 "improved enemy AI in battle" on (B+5 = 2), or for the player side's auto
  actions, a target is killable when `HP ≤ actionsLeft × dmg`.
- In AI-vs-AI simulations B+5 = 0.

**No randomness.**
- The picker would add `rand((max − min) × B+9 / 100)` from the game's own LCG (4832fc), but
  B+9 is 0 at every caller.
- CrazyAI (4ed3d4) is written by the ini loader and **never read**.
- Delphi `Random` has no caller in battle code.
- So battles are fully deterministic.

**Pre-simulation.** Battle setup (48b75c) first plays a complete AI-vs-AI copy of the battle,
then restores the state. It keeps only each side's damage counter, probably as a strength
prediction. Players never see it.

## 5. Turn limit and surrender

**End check** (end of 48a5c4, 48b67b–48b751). It runs after **every action**. The battle ends if:
- a side has no units, or
- `turn ≥ BattleEndTurn`, or
- one side's every remaining unit has `Surrender > 0`.

The turn counter goes up when a turn starts (4840ec). So with BattleEndTurn = 25 the battle
stops after the **first action of turn 25**: 24 full turns plus one action. **code**

**Turn-limit outcome** (4c50ec, 4c5274). There is **no draw branch**.
- If the player's side still has any unit, the **victory** path runs: gold loot at
  `enemy gold / VictoryGoldDiv`, trophies, and the lord and army handling for the enemy.
- Only a player side with 0 units is a defeat.
- So reaching the turn limit alive counts as a win. **code** for the branch. The enemy
  survivors are handled like a beaten army; that detail is **data**-level.

**Surrender.**
- **Values:** each battle unit carries its type's `Surrender` value (copied at 4983c6).
- **Trigger:** when every remaining unit of a side has Surrender > 0, that side is flagged at
  the end-of-action check (48b6ba–48b70a). Examples are priests, nuns, witches, mages and
  townsfolk.
- **Result** (48bfb4–48c091):
  - The flagged side's units are all removed (HP 0, count 0).
  - The **sum of their Surrender values** becomes mana for the winner. This is the "VictoryMana"
    text: the spared troops pray for you.
  - Units killed before the surrender give **no** mana.
- **Either side:** the rule applies to the player's side too. With only surrender-capable units
  left (a hero has Surrender 0, so only once the hero is down), the player's side is removed and
  it is a defeat. **code**
- **Why a lone unit gives mana:** a garrison of one Surrender = 20 unit surrenders after the
  first action and gives +20 mana. This matches the footage.

## 6. Columns and wide row

- **Column count.** The global at 4ed044 is 6 when `[Options] OptValue11 = 1`, else 4 (read at
  4b8a46). The width is stored in the save header at new game (4b25a2) and restored on load
  (4b77f3). **code**
- **Vanilla (4 columns):** 3 rows × 4 columns, 12 cells.
- **Wide (6 columns)** (48395c). The grid blocks cells by filling them with −1:
  - front row: 6 cells;
  - back row: 4 cells, columns 2–5;
  - reserve: 2 cells, columns 3–4.
  - That is still 12 cells. Blocked cells can be neither entered nor targeted. **code**
- **Preferred column order** (AI picking and placement):
  - 4 columns: 3,2,4,1 (tables 4ed030 and 4ed034);
  - 6 columns: 4,3,5,2,6,1 (4ed018 and 4ed01c). **code**

## 7. Community bonuses: what the code does

Full detail is in notes/bonuses.md. Everything here is **code** unless marked. Where the code,
the in-game text (Rus_DiscordTimes.ini, BonusN) and the changelog disagree, the numbers below
are the code's.

**Enum.** The token table gives these values: 22 Hunger, 23 Berserk, 24 Exhaustion, 25 Drying,
26 CtrPoison, 27 Suicide, 28 Caster, 29 Splash, 30 Fortify, 31 Dominate, 32 PoisonS,
33 Concentration, 34 Potent, 35 Stun, 36 FirstShot, 37 Bastion, 38 Flying, 39 Bleed,
40 PreventiveStrike, 41 Flock, 42 ArmorBreaker, 43 NoHeal, 44 FasterAttack,
45 PoisonArmorIgnore, 46 HoldLine, 47 Neutralize, 48 KillingStrike, 49 BloodThrist,
50 Assault, 51 EternalGift, 52 FateGift. The parser is at c255a1, c25af7, c27d94 and c28e85.
This matches src/dt/data.rs.

**One bonus per unit.** The unit has a single bonus byte. Each worn item that has a bonus
**overwrites** it; the last slot with one wins (4919f0–491a4f).

**Where the hooks run.**
- Three hit paths each have their own chain of after-hit hooks: melee (48b2f6), shot (48b214)
  and hostile magic (48ac7e).
- In the magic path the "damage" tested is the spell's power, so the "damage > 1" effects also
  fire on curses.
- The turn-start hooks sit in 4840ec, after the per-turn reset.

| Bonus | Original rule | Where |
|---|---|---|
| Hunger | **Melee** kill: heals to full. Also, at a turn start from turn 2, if the total number of living units changed since the last check, a Hunger unit heals to full. The "last seen" counter is shared, so only the first Hunger unit benefits. Shot and spell kills do not heal. | c252e9, c25370 |
| Berserk | Attack modifier **set** to `AB × 75 × (maxHP − HP) / maxHP / 100`, up to +75% of AB. Recomputed at turn start and whenever it is hit. It overwrites a blessing's attack. | c256c8, c25777, c2581b, c258bf |
| Exhaustion | Each of its hostile spells lowers all 3 protections of the target by **10 points**, cumulative, not below 0. The text says 10%, the changelog 15. | c26222, c27ec7 |
| Drying | After its hostile spell: `max(1, 8% maxHP)` extra, ignoring protection. | c2596b |
| CtrPoison | A **melee** or long striker gets **regen −20**, stacking per hit. Shots and spells do not trigger it. | c25c43 |
| Suicide | Dies after any hostile action of its own. | c2633c, c262a1, c261c1 |
| Caster | World spells only. | c25ccc |
| Splash | The first hit uses 80% of attack or power. The same action is then repeated at 40% on the target's same-row neighbours at c±1. For melee the neighbour must also be within 1 column of the attacker. Works for shots, hostile magic, **and heals and blessings**. Only in the interactive battle (flag 4ed424), not in simulations. | c26f2d, c270e5, c2718d, c27274, loop c26fd2 |
| Fortify | From turn 2, at turn start, the defence modifier gets `max(1, DefenceBlow×25%) × min(turn − 1, 5)`: +25% per turn after the first, up to +125% from turn 6. It is based on DefenceBlow and counts for both defences. The text says 20%/100%. | c25a1d |
| Dominate | **No effect.** Its routine (c263d2) is unreachable: the entry is jumped over and nothing branches to it. | c263cb |
| Poison (vanilla #17) | Melee or shot damage > 1 sets regen to **−20**, replacing the unit's own regen. Community: mages poison too, when power after protection > 15. The text says 15%. | 48b2bd, 48b3bc, c259da |
| PoisonS | Regen set to **−25**. Same triggers as Poison. | c2639c, c26301, c26d5c |
| Concentration | The drain is added instead of subtracted, with no cap. The floor still applies. | c2851a |
| Potent | Its strikes and curses skip protection **and** the nature multipliers (no ×2 on Undead, no ×¾ on Elementals). GodAnger and GodStrike still add. | c26e78 |
| Stun | Every hostile hit or spell, with no damage test, lowers the target's initiative modifier by **30% of its current initiative**, cumulative within the turn. It is reset at the next turn start. The text says 35%, the changelog 25%. | c26e1f, c27e6e, c289ae |
| FirstShot | Turn 1: +30 initiative, and +30 more with building defence ≥ 10. The same as vanilla Artillery. | c28935 |
| Bastion | At **every** turn start it doubles its own AB, AS, DB and DS, compounding (×2, ×4, ×8…). There is no building check, no damage halving and no army +10. Almost certainly a bug. | c26ebb |
| Flying | From row 1 **or row 2**, it can melee the three enemy **front** cells c±1. It cannot reach the enemy back row and cannot act from the reserve. A Flying shooter or mage only gains a melee option. | c29150 |
| Bleed | A hit with damage (or power) > 1 sets the target's bleed to 75, a maximum that does not stack. **Each time the bleeding unit starts an action** it loses `(AB + AS + MP) × 75%` HP; if that kills it, the action is cancelled. It lasts all battle. | c29235, c2a53c (hooked at 48a677) |
| PreventiveStrike | Before an enemy's **melee** on it, it strikes first: melee if it has AB, else a shot. Before a **shot or spell** on it, it shoots first, but only if it has AS. Every attack, no limit. If the attacker dies, the attack is cancelled. | c2a181, c28aea |
| Flock | Turn start: the attack modifier ±25% of AB (or of AS when AB is 0), depending on which army record has more units. That these are the **start-of-battle** sizes is medium confidence. Equal sizes: nothing. | c29d0f |
| ArmorBreaker | A hit with damage > 1: the target's DB and DS ×**0.75**, cumulative, for the battle. | c29282 |
| NoHeal | Any hit marks the target for the battle. A marked unit can be neither **healed nor blessed** (no code-9 cell), and positive regen is set to 0. Vampirism and Hunger are **not** blocked. | c2967a, c2962c |
| FasterAttack | +1 action on turns 1 and 2. | c29c6b |
| PoisonArmorIgnore | Pierces like ArmorIgnore (building defence and, for shots, Row2Def still count). A hit with damage > 1 sets regen to `min(regen, −10)`. | c2a27c, c2a3bf |
| HoldLine | No code. | — |
| Neutralize | Every hit clears the target's bonus byte for the rest of the battle, with no damage test. | c29411 |
| KillingStrike | After damage > 1, the target dies if its HP ≤ 25% of max. It is checked before FateGift, so FateGift can still save the target. | c2930a |
| BloodThrist | A kill in any path gives +1 action. | c2a296, c2a2f9, c2a35c |
| Assault | Turn 1 only: AB, AS, DB and DS ×2 when the enemy's first unit has building defence ≥ 10. Damage taken ×**2/3**. The damage test reads a misaligned field (medium confidence). | c29c97, c2a3d9, c2a403 |
| EternalGift | See section 1: base-stat changes that last the battle and stack. The Life blessing lowers defence (bug). | c29e59 … c2a0f8 |
| FateGift | Once per battle, a hit that leaves it at HP ≤ 0 instead:<br>• refills its actions;<br>• gives all protections +20 and regen +20;<br>• raises max HP by 20% with a full heal;<br>• gives the initiative modifier +5 (this turn).<br>The bonus is then erased. Poison and bleeding deaths are not saved. | c2937f |
| Evasion (unit field) | The last step of physical damage, counter and preventive strikes included: `max(1, dmg × (100 − E) / 100)`, from a table per unit type. | c2a802 |
| Garrison (Community fix) | At each turn start, if building defence is **exactly 10**, the attack modifier gains AttackShot. That also counts for melee. | c2651a |

## 8. Other battle rules

**Turn order** (489ca0, 4840ec). **code**
- **Threshold scan.** The battle keeps a descending initiative threshold T.
  - On turn 1, T starts at **75**. On later turns it starts at the threshold at which the first
    unit acted the turn before.
  - At each T it scans side 1 (the player) units in list order, then side 2.
  - A unit acts if `initiative + initModifier ≥ T` and it has actions left. It uses all its
    actions in a row before the scan moves on.
  - After a full scan T drops by 1. At T = 0 a new turn starts.
- **Ties** go to the **player's side**, then to list order (army order), whoever attacked.
  Units with effective initiative ≤ 0 never act.
- **Initiative changes** made mid-turn (bless, curse, Stun) take effect at once.
- **"Attacker +1".** Battle setup adds +1 initiative to **side 1 = the player**, always
  (48b917). There is no code that gives it to the enemy.
- **Artillery** is not "always first". It gets **+30 initiative on turn 1 only**, and +30 more
  when its building defence is ≥ 10 (484365–4843d8).
- **+1 action on turn 1:** HorseAtack, OldVampirsGist and FastDead (48431a).

**Start of each turn** (4840ec), for every unit:
- attack, defence and initiative modifiers are reset to 0, and the blessed and cursed flags are
  cleared;
- actions are set to Manevres, and initiative to its base;
- the reserve flag is set to 1;
- from turn 2: magic power drains (section 1), then **regeneration and poison**:
  `HP += round(maxHP × regen / 100)`, capped at max HP. A unit that reaches 0 or less dies and
  is removed (4846c1–4847e9). There is no minimum of 1, and the rounding is Delphi's (to even).

**Poison (vanilla)** (48b299, 48b398). **code**
- A Poison attacker's physical hit of more than 1 damage sets the target's regen field to
  **−20**. The target then loses **20%** of max HP per turn, and its own regeneration is replaced.
- The in-game text says 15%. The Community changes are in section 7.

**Vampirism** (48b3d4). **code**
- After each physical hit the attacker heals `Vampirizm × dmg / 100`, where dmg is the
  **uncapped** computed damage (overkill counts). It does not apply against Undead or Elemental
  targets.
- Magic: Death-school strikes only (section 3).

**Counterblow** (48b49f–48b579). **code**
- The constant at 4ed384 is 1. After a **melee or long strike** (not a shot or spell), a
  surviving Counterblow target strikes back with melee damage (kind 4).
- There is no warrior check: a unit with AttackBlow 0 counters for 1.
- The counter does not trigger vampirism or poison. A counter that kills the attacker removes it.

**On a kill** (48a3f0). **code**
- **DeathCurse target:** the killer dies.
- **Ghost target:** the killer dies only if its **ProtectDeath** (+0x44) is below
  `30 × the Ghost's Manevres` (+0x50). With 1 action, a killer with Death protection ≥ 30%
  survives. So the vanilla Ghost curse is conditional. The field reading is **code**.

**Garrison** (side build 49861d). **code**
- In a building with defence ≥ 10, a Garrison unit's AttackBlow, DefenceBlow and DefenceShot are
  doubled at battle start. **AttackShot is not doubled.**
- The Community "garrison works for shooters" patch is in section 7.
- The ×2/3 damage taken also needs building defence ≥ 10.

**Hero 1 HP** (4906a0). **code**
- After the battle, if any unit of the player's army has HP > 0 and the hero (unit 1) has 0, the
  hero's HP is set to 1. The same fix appears in an event path (4b0baf).
- Whether AI lords get it is **unknown**.

**Knight:** −20% physical damage in this build (section 0).

**Unpaid units.** Units with the "not fighting" flag are left out of the battle side (49855c
skips them). **code**

**First-turn rules.**
- SpearDefense ×3 melee defence on turn 1 only.
- FasterAttack is Community, see section 7.

## Razdor now → original (src/rules/battle.rs, formation.rs, game.rs)

| # | Topic | Razdor now | Original (this exe) | § |
|---|---|---|---|---|
| 1 | Buff/curse duration | 3 turns including the cast (`EFFECT_TURNS`) | Until the start of the next turn, since every modifier is reset each turn | 1 |
| 2 | Stacking | One blessing and one curse slot, a new one replaces the old | Additive modifiers. Each ally can be blessed once per turn (heals still allowed); each enemy is cursed once per turn, then struck | 1, 3 |
| 3 | EternalGift | Lasts the battle, same values | Changes base stats: lasts the battle, stacks per cast, and the Life blessing lowers defences (bug) | 1 |
| 4 | Magic drain | Only above the floor: `max(floor, P − dec)` | If MP > 0: `max(MP − drain, floor)`, which raises weak casters to the floor; Undead Death casters get floor +25 | 1 |
| 5 | Concentration | +10% of base per turn, capped at 2× | Adds the drain value per turn, no cap | 1 |
| 6 | Hostile mage action | Strike for Life, curse otherwise; the right click picks the other | No choice for the player or the AI: curse if the target has no negative modifier, else strike, for every school | 3 |
| 7 | Friendly mage | Heal or bless options | Automatic: heal if wounded and heal > 0, else bless. Targets: wounded or not yet blessed. Reserve casters target only the reserve | 3 |
| 8 | Undead casters | — | Elemental or Death curses by an Undead caster also drain `(P/CurseMainSpell)/2 + 1` HP to the caster | 3 |
| 9 | Vampirism | Any damage, capped at target HP, any school, any target | Physical hits and Death strikes only; uses the uncapped damage; not against Undead or Elemental; not blocked by NoHeal | 3, 8 |
| 10 | Life curse | `defence −(1 + 3P/(2·CMS))` | `defence −(P / floor(2·CMS/3) + 1)`, i.e. P/3 + 1 | 3 |
| 11 | Into the reserve | From the back row only, any time | From the front **or** back row, to any empty reserve cell; at most one reserve transition (in or out) per unit per turn | 2 |
| 12 | Collapse timing | After every action and move, at turn start and at begin | After a unit's death, and for the actor's side when it ends its actions; a voluntary move collapses only after the mover's last action | 2 |
| 13 | Reserve collapse | Reserve moves to the front, keeps its actions | Reserve moves to row 1 and **loses its remaining actions** that turn | 2 |
| 14 | Wide formation | `WIDE` = 2 × 6 with no reserve | Front 6, back 4 (columns 2–5), reserve 2 (columns 3–4); still 12 cells | 6 |
| 15 | Turn order | Sorted list; ties go to the attacker | Descending threshold scan (turn 1 starts at 75). Ties go to the **player's side**, then army order. Initiative changes apply at once. Initiative ≤ 0 never acts | 8 |
| 16 | Attacker +1 initiative | To `attacker` | Always to side 1 = the player | 8 |
| 17 | Artillery | Always first; pierces melee and shots | +30 initiative on turn 1 only (+60 with building defence ≥ 10); pierces **shots** only | 0, 8 |
| 18 | Piercing set | ArmorIgnore, both vampire gifts, Artillery, PoisonArmorIgnore, for every kind | Melee: ArmorIgnore, PoisonArmorIgnore, VampirsGist, OldVampirsGist. Shots: ArmorIgnore, PoisonArmorIgnore, Artillery | 0 |
| 19 | Piercing vs Row2Def | `def = building` (drops Row2Def) | Unit defence becomes 0; Row2Def (for shots) and building defence are still added | 0 |
| 20 | SpearDefense and long strike | ×3 and ÷2 applied to own + building (+ Row2Def) | Applied to the unit's own defence (plus modifier) only; building defence is added afterwards | 0 |
| 21 | Knight | ×90/100 | ×80/100 (set when `[GlobalOptions]` loads, 4e4501); the flag comes from the army's hero class, not a living hero | 0 |
| 22 | Counterblow | Warriors only | Any Counterblow unit (AB 0 counters for 1), after melee or a long strike | 8 |
| 23 | Ghost | Killer always dies | Killer dies only if its ProtectDeath < 30 × the Ghost's Manevres | 8 |
| 24 | Garrison | ×2 on AB, AS, DB, DS | Base code: ×2 on AB, DB, DS (building defence ≥ 10). Community: +AS to the attack modifier each turn, only when building defence is exactly 10 | 8, 7 |
| 25 | Regen | `max(1, …)`, positive only, separate from poison | `round(maxHP × regen/100)`, no minimum; poison **sets** regen negative, replacing it | 8 |
| 26 | Poison | 15%; strongest poison counts | Regen −20 (Poison), −25 (PoisonS), `min(regen, −10)` (PoisonArmorIgnore); CtrPoison −20 stacking; mages poison when power after protection > 15 | 7 |
| 27 | Turn limit | 25 full turns, then a stalemate (player withdraws) | Ends after the first action of turn 25. **Victory** if the player has any unit left (normal loot) | 5 |
| 28 | Surrender | `game.rs`: every beaten enemy gives its Surrender in mana | A side whose remaining units **all** have Surrender > 0 gives up at once. Only those units' Surrender sum becomes mana; units killed earlier give none. Applies to the player too (defeat) | 5 |
| 29 | Bonus count | A list of bonuses per unit (`has_any`) | Exactly one bonus byte; each worn item with a bonus overwrites it (last wins) | 7 |
| 30 | AI | Heal < 50%, kill the most dangerous, curse, fewest hits, heal or bless; step-forward moves | Scored priorities (retreat non-warriors, melee `dmg×round((R+1)M)`, shots, school magic, moves). Kill test depends on OptValue9. Never moves into the reserve. No randomness | 4 |
| 31 | Hunger | Any kill heals to full | Melee kills only; plus a turn-start heal when the living-unit count changed (shared counter) | 7 |
| 32 | Berserk | Damage × (2·max − hp)/max | Attack modifier = AB × 75% × missing/max (up to +75% AB), overwrites a blessing's attack | 7 |
| 33 | Exhaustion | −15 | −10 points on all three protections | 7 |
| 34 | CtrPoison | 15% poison on a melee striker | Striker's regen −20, stacking per hit | 7 |
| 35 | Splash | 80/40 on physical hits, row neighbours | 80/40, also on shots, spells, heals and blessings; a melee neighbour must be within 1 column of the attacker; interactive battles only | 7 |
| 36 | Fortify | `def × (100 + min(25·turn, 125))/100` from turn 1, on own defence | From turn 2: flat `max(1, DB/4) × min(turn − 1, 5)` added to both defences | 7 |
| 37 | Dominate | ×1.25 vs smaller max HP (guess) | No effect (dead code) | 7 |
| 38 | Potent | Skips protection | Skips protection **and** nature multipliers | 7 |
| 39 | Stun | Once per target, base initiative × 3/4 for the battle | Every hostile hit or spell: initiative modifier −30% of current initiative, cumulative, this turn only | 7 |
| 40 | FirstShot | First on turn 1 | +30 initiative on turn 1 (+30 more with building defence ≥ 10) | 7 |
| 41 | Bastion | ×3 inside, half damage, army +10 | AB, AS, DB, DS doubled at every turn start, compounding, no building check (bug) | 7 |
| 42 | Flying | Any enemy in the front or back row, from any active row; shooters and mages unblocked | Melee on the three enemy front cells c±1 from row 1 or 2; nothing else changes | 7 |
| 43 | Bleed | Half the wound again next turn | Bleed 75: each action start costs `(AB + AS + MP) × 75%` HP, all battle; a kill cancels the action | 7 |
| 44 | PreventiveStrike | Strikes (warrior) or shoots first before any physical attack | Before melee: melee if AB, else a shot. Before shots and spells: shoots only if it has AS | 7 |
| 45 | Flock | Damage ±25% by living counts | Attack modifier ±25% of AB (or AS) by army sizes at battle start (medium confidence) | 7 |
| 46 | ArmorBreaker | ×0.7 | ×0.75 on DB and DS, cumulative | 7 |
| 47 | NoHeal | Blocks heals, regen, vampirism, Hunger | Blocks heal **and** bless targeting, zeroes positive regen; vampirism and Hunger still work | 7 |
| 48 | KillingStrike | Below 25% | HP ≤ 25% after damage > 1; checked before FateGift | 7 |
| 49 | Assault | While storming: ×2 all battle, damage taken ×0.7 | Turn 1: stats ×2 when the defender's first unit has building defence ≥ 10; damage taken ×2/3 (medium confidence) | 7 |
| 50 | FateGift | Full HP, attack and defence +25% | Actions refilled, protections +20, regen +20, max HP +20% with a full heal, initiative +5 this turn; not on poison or bleed deaths | 7 |

## Unknowns and open points
- **Enemy survivors at the turn limit.** Exactly what happens to them in the victory branch;
  4c5622 sets the enemy army record's count to 1, which looks like leader-only.
- **AI lords.** Whether AI lords get the hero-1-HP rule.
- **AI magic scoring.** Life, Death and Elemental magic scoring is medium to low confidence
  (notes/ai.md).
- **Auto-arrange.** How auto-arrange (483b3c) fills the rows was only partly decoded.
- **Low-confidence hooks.** Flock's army-size source and Assault's damage test are medium
  confidence. The Manevres-0 hook at c25b63 is low.
