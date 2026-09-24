# Discord Times — mechanics and data-file notes for the Razdor remake

Written in our own words from the game's shipped help, the editor manual, the Community
Update notes and, where those say nothing, a reading of `DiscordTimes.exe`
(Delphi, Community Update 1.2 / game 1.8.1, `.mod` patch section included).

Confidence tags:
- **[doc]**: stated in the shipped help, editor manual, modder notes or UI strings.
- **[exe]**: read from the executable's code. The formula is reliable, but what a field
  means is our interpretation.
- **[inf]**: inferred from data values or naming. Plausible, not verified.
- **[unk]**: unknown.

Sources used: `Help/Как Играть.htm`, `Help/Документация к Редактору Сценариев.doc`,
`Дополнительные файлы/*` (modder note, changelog, new-editor guide, ID notebooks),
`Rus_*.ini`, `_Global.ini`, `DTMapEdit_Rus.Ini`, `Rus_MapEdit.ini`, `DiscordTimes.exe`.

---

## 0. Ini file format (all data files)

- Encoding: Windows-1251, CRLF. The Community Update extras (`Дополнительные файлы`) are already UTF-8. [doc/inf]
- `//` starts a comment line. `[Section]` headers. `Key=Value`. Values may be empty
  (for example `TimeWork=` means "instant / no duration"). [doc]
- `Rus_Units.ini` / `Rus_Artefacts.ini` sections are named `[<GlobalIndex> <Name>]`, and the
  entity is keyed by `GlobalIndex`. Every spell section in `Rus_Spells.ini` has the same
  name, `[Заклинание]`, so a spell's ID is its **order in the file**. The file ends with an
  empty template section and `[MapEditorSpecialOptions] Generated=1`, which must not be
  deleted (the file says so). [doc]
- Maps and events refer to units, items and spells by numeric ID, so the load order must be kept. [inf]
- Parser: `ini_parse.py` in the scratch dir. It handles duplicate section names and returns a list.

---

## 1. Units (`Rus_Units.ini`, 102 entries)

### 1.1 Fields

| Key | Meaning | Tag |
|---|---|---|
| `GlobalIndex` | Unit type ID (1–102). IDs 1–3 are the three hero classes. | doc |
| `Name`, `Descript` | Display name and card text. | doc |
| `IconIndex` | Portrait index into `Graphics/Objects/Persones.ugs` / `Icons.ugs`. | inf |
| `Cost` | Hire price in gold. It is also the base for wages, healing, resurrection and XP value. | doc/exe |
| `CostMultipler` | Editor label "Коррекция" next to "Сила" (strength). Percent correction of the unit's **tactical cost** (strength estimate used for AI and XP): `tactical × CostMultipler / 100`. Values 50–100. | exe |
| `CostGoldDiv` | Divisor stored per type (1–5; undead 2–5). Not used by the wage code. Probably divides the unit's personal money, which the winner takes as loot. | inf |
| `StartExpirience` | XP needed for the first level-up of this class. | exe |
| `LevelMultipler` | Percent growth of the XP requirement per level (140 for most units, 150/160/170 for heroes). | exe |
| `Hits` | Max HP. | doc |
| `AttackBlow` | Melee attack. If >0 the unit can fight in melee. | doc/exe |
| `AttackShot` | Ranged attack. If >0 the unit is a shooter. | doc/exe |
| `MagicPower` | Magic power ("сила магии"). If >0 the unit is a caster. | doc/exe |
| `DefenceBlow` / `DefenceShot` | Flat defence against melee / ranged attacks. | doc/exe |
| `ProtectLife/Death/Elemental` | **Percent** resistance to hostile magic of that school (editor: "% Защита от магии …"). | doc/exe |
| `Initiative` | Turn order. Higher acts first. | doc |
| `Manevres` | Actions per turn ("количество действий"). 1–3. | doc |
| `Regen` | % regeneration (editor "% Регенерация"). When it applies (per battle turn or per day) is unverified. | doc/unk |
| `Vampirizm` | % vampirism: part of damage dealt comes back as HP. | doc/inf |
| `Magic` | `LifeMagic` / `ElementalMagic` / `DeathMagic` (editor: Жизни / Стихий / Смерти). Magic school. Warriors can carry one too (Paladin, Church Guard). | doc |
| `MagicDirection` | `ToAll` / `ToEnemy` / `ToAlly` (editor "На всех / На чужих / На своих"): whether the caster targets enemies, allies or both. | doc |
| `Nature` | Creature type. Editor order: Normal(0, default), `Undead`(1), `Elemental`(2), `Rogue`(3), `Animal`(4), `Hero`(5), `People`(6). It changes magic effects (see 3.3). Community Update: `Nature=Elemental` units are **paid in mana** (hire, heal, resurrect, daily wage). | doc/exe |
| `Bonus` | One special ability (enum, see 1.3). | doc |
| `Surrender` | Editor label "Плен" (captivity). Present on priests, nuns, mages and townsfolk (10–100). Probably the chance or amount that makes a beaten unit surrender. Surrendered enemies give the victor **mana** ("they pray for you"). | inf |
| `NextUnitN`, `NextUnitNLevel` (N=1..3) | Upgrade tree: the unit can be promoted to class `NextUnitN` once it reaches level `NextUnitNLevel` (always 1 in vanilla). | doc/inf |
| `d-<Stat>` | Stat gain per level for Hits, AttackBlow, DefenceBlow, AttackShot, DefenceShot, MagicPower, Initiative, Manevres, Protect*, Regen, Vampirizm. | doc |
| Community: `Evasion=1..100` | "Неуязвимость": ignores this % of physical damage, applied last: `dmg = max(1, dmg·(100−Evasion)/100)`. | doc/exe |
| Community: `MinMagicPower`, `ManaDrain` | Per-unit floor and per-turn loss of magic power in battle (they override the globals). | doc |

The exe also recognises some tokens that vanilla data never uses: `Nature=Animal/Hero`,
bonuses `AddPayment`, `FlankStrike`, and many Community bonus tokens (list in 1.3). [exe]

### 1.2 Unit classes

There is no class field. The role follows from which stats are above zero. [exe]
- **Warrior**: `AttackBlow>0`. Melee only, and only from the front row.
- **Shooter**: `AttackShot>0`. Ranged.
- **Mage**: `MagicPower>0` with `Magic` and `MagicDirection`. Heals and blesses allies, and strikes or curses enemies (see 3.3).
- Some units have both melee and magic (Paladin, Church Guard, Inquisitor).

Heroes: Knight (id 1, warrior), Archmage (id 2, Elemental mage, ToEnemy), Ranger (id 3, shooter, Regen 5). [doc]

Hero class bonuses [doc; knight part also exe]:
- **Knight**: the army takes 10% less **non-magic** damage (`dmg·90/100`, hard-coded 90). The knight can also use global spells, but slowly.
- **Archmage**: casts global spells twice as fast and for 50% of the mana. The army gets no other bonus. In battle the archmage slows enemies (fewer actions and less initiative).
- **Ranger**: the army moves 20% faster on the map, and wounded units heal 20% of max HP per day.

### 1.3 Bonus enum (value = UI `BonusN` index)

| # | ini token | Effect (own words) | Tag |
|---|---|---|---|
| 1 | SpearDefense | Long weapon: on battle turn 1 its melee defence is tripled. | doc+exe |
| 2 | HorseAtack | Fast attack: +1 action on the first battle turn. | doc |
| 3 | ArmorIgnore | Piercing: its attacks ignore the target's defence (building defence still counts). | doc+exe |
| 4 | ArmyMedic | Medic: the whole army heals 15% of HP per day. | doc |
| 5 | Merchant | Trader: selling gives +50%, buying costs −30%. | doc |
| 6 | DeathCurse | Whoever kills this unit dies at once. | doc |
| 7 | GodAnger (UI "Кара") | +10 damage on top, ignoring all defences (also adds to magic strikes). | doc+exe |
| 8 | GodStrike (UI "Гнев") | +20 damage on top, ignoring all defences. | doc+exe |
| 9 | Unvulnerabe | Every physical hit does exactly 1 damage. | doc+exe |
| 10 | VampirsGist | Dark gift: ignores the target's defence. Incoming physical damage ×2/3. | doc+exe |
| 11 | OldVampirsGist | As 10, plus +1 action on turn 1. | doc |
| 12 | Evasive | Incoming physical damage ×2/3 (UI says "70%"). | doc+exe |
| 13 | Ghost | Immune to physical harm (takes 1 per hit), and its killer dies. | doc+exe |
| 14 | Artillery | Barrage: always acts first in battle, and its damage ignores defence. | doc |
| 15 | Garrison | Inside a castle or fort: stats ×2, and physical damage ×2/3 (code: needs building defence ≥10). | doc+exe |
| 16 | AddPayment | Rear service: the army's wages −30%. | doc |
| 17 | Poison | After a solid hit the target loses 15% HP every battle turn. | doc |
| 18 | Dead | Corpse: shots do only 30% damage. | doc+exe |
| 19 | FastDead | As 18, plus +1 action on turn 1. | doc+exe |
| 20 | Counterblow | Hits back when struck in melee. | doc |
| 21 | FlankStrike | Doubles its attack on a "flank" strike (the long strike through empty cells, see 2.3). | doc+exe |
| 22+ | Community tokens | Hunger, Berserk, Exhaustion, Drying, CtrPoison, Suicide, Caster (−20% spell time and cost), Splash (80% to the target and 40% to its neighbours), Fortify, Dominate, Concentration, PoisonS (25%/turn), Stun, Potent (magic ignores protection), FirstShot, Bastion, Flying (can hit any row), Flock, Bleed, HoldLine (does not work), ArmorBreaker, PoisonArmorIgnore (#45), FasterAttack, NoHeal, PreventiveStrike, Neutralize, KillingStrike (<25% HP dies), BloodThrist, Assault (#50), EternalGift (buffs last the whole battle), FateGift. | doc |

### 1.4 Experience and levels

- XP needed to go from level L to L+1: **`StartExpirience × (LevelMultipler/100)^L`**, rounded.
  The code builds this as a geometric series and takes the difference of two partial sums. [exe]
  Example: a Militia unit needs 60, then 84, then 118 …
- Each level adds the `d-*` values to the stats. [doc]
- Promotion (`NextUnit*`): a unit with enough XP can instead switch to a class from its
  upgrade tree. The new class has its own `StartExpirience`. Some classes are final
  ("will only improve by levels"). Only units at the starting level can be hired. [doc/inf]
- Community cap: at most 5256 XP gained at once, and the overflow is no longer wiped. [doc]
- **XP from a battle** [exe, simplified]:
  - `ratio = enemyTacticalCost / ownTacticalCost`
  - `k = 1 + (ratio−1)·ExpCorrection/100` when ratio ≥ 1, else `k = 1 − (1−ratio)·ExpCorrection/100`, clamped to [0.25, 4]
  - `pool ≈ MainExpCorrection/100 · k · (tactical cost of the enemy destroyed)`, plus a smaller term from `enemyTactical/20` and the damage exchanged
  - The pool is split among the side's units. Weights depend on the row (front row weighs more: `4−row`) and on each unit's contribution. Each unit gets at least 1.
  - Then `HeroExpirienceModificator` (50% for the player) or `AIExpiriencePercent` (100%) is applied, along with the editor's per-army "XP correction". [doc]
  - The help confirms: a harder fight gives more XP, and a bigger army means less XP each. [doc]

### 1.5 Wages (daily upkeep) [exe + doc]

Let `C = Cost`. The code has two branches, chosen by a per-unit hiring kind (1 or 2). Which
units fall in which kind is **[unk]**. Razdor's guess: kind 2 (mercenaries) for `Nature=Rogue` units, kind 1 for the rest (section 8).
- Kind 1: `wage = round(C / CostRecrutDiv × f)`, where `f = 0.25` if C≤50, `0.5` if C≤100, `0.75` if C≤150, and `1` otherwise. `CostRecrutDiv=2`, so a Militia unit (C=50) costs 6 gold/day.
- Kind 2: `wage = C / CostMercenaryDiv` (=2).
- The hero is free. A unit left in a garrison stops asking for pay from its second day there. [doc]
- When gold runs short, the unpaid units **do not fight** until they are paid. A village
  innkeeper option can restore them. `MaxTimeNotUpkeep=10080` minutes (7 days): how long an
  army keeps going unpaid, probably before the units leave. [doc/inf]
- Rear Service (`AddPayment`) −30%. Elemental-nature units are paid in mana (Community). [doc]

### 1.6 Healing and resurrection [doc + exe]

- Heal in a town, a castle (own or friendly) or a church. Cost ≈ `Cost × HealingConst% ×
  (missing HP / max HP)`, where `HealingConst=50`. It takes `HealingTime=60` minutes of game time. [doc/exe/inf]
- Resurrection only in a **town or church**: `Cost × ResurectConst%` (300% = 3× cost). Allowed
  within `MaxTimeResurection=10080` minutes (7 days) of death. Otherwise the body can only be buried. [doc/exe]
- Garrisons heal `GarrisonAutoHeal=10%` of HP per day. Medic units give 15% per day, and the Ranger hero 20% per day. [doc]
- Potions (see 4) heal instantly on the army screen.

---

## 2. Battle

### 2.1 Formation [doc + exe]

- Each side has **3 rows × N columns**: row 1 = front (swords icon), row 2 = back (bow icon),
  row 3 = reserve (tent icon). **N = 4 in vanilla.** It becomes 6 only with the Community
  option "wide front row" (`OptValue11`). An army holds at most 12 units (4×3 = 12). [doc/exe]
- The front row screens the back row. When every front-row unit is dead, the back row
  becomes the front. [doc]
- The back row gets **+`Row2Def`=5 defence against shots only**. [doc+exe]
- Units in the reserve cannot be attacked and cannot act. The reserve is for pulling wounded
  units out and bringing fresh ones in. [doc]
- In your own building, defence also gets the building's extra defence (set per garrison in the editor). [doc/exe]
- Preferred column order (AI and auto placement) goes from the centre outwards: 3,2,4,1 for
  4 columns and 4,3,5,2,6,1 for 6. [exe]

### 2.2 Turn order and actions

- The first move goes to the side with the highest-initiative unit. Units act by initiative,
  highest first. The attacking army gets **+1 initiative**. [doc]
- Each unit has `Manevres` actions per turn. An action is an attack, a heal or bless, or a
  move. Space skips the rest of the unit's turn. [doc]
- Movement [exe]:
  - From the front or back row, a unit moves to an empty cell in column c−1, c or c+1 of the front or back row.
  - A unit in the reserve can move to any empty cell of the front or back row.
  - Moving to any empty reserve cell is allowed only when a certain per-unit flag is set. The flag's condition is [unk].
- The battle ends when one side has no units left, or when the turn counter reaches **`BattleEndTurn`=25**. What happens in that undecided case is [unk]. [exe]
- **You cannot retreat** or leave to the map before the battle ends (only "exit to menu" or "restart"). Pulling units into the reserve is the only way to save them. [doc]

### 2.3 Who can hit whom (enemy cells) [exe]

Let the attacker stand in column c.
- **Warrior** (needs `AttackBlow>0`, must be in **row 1**):
  - It can hit occupied enemy **front-row** cells at c−1, c and c+1 (a normal strike).
  - If all three are empty, it gets a **long/flank strike** at the nearest occupied enemy
    front-row cell to the right (scanning c+2, c+3 …) and the nearest to the left (c−2, c−3 …).
    That strike **halves the target's defence**, and the FlankStrike bonus doubles the attack.
  - A warrior in row 2 or 3 cannot attack.
- **Shooter** (`AttackShot>0`):
  - From **row 2** it can target any enemy in rows 1–2.
  - From **row 1** it can target any enemy in rows 1–2 only if the three enemy front cells
    c−1..c+1 are all empty. Otherwise it can target only those adjacent front-row enemies.
- **Mage, hostile** (`MagicPower>0`, direction includes enemies, i.e. not `ToAlly`):
  - From row 2 it can target any enemy in rows 1–2.
  - From row 1 it can do so only if the three opposite cells are empty. Otherwise it cannot cast.
- **Mage, friendly** (direction includes allies): it can target an own unit in row 1 or 2
  that is wounded, or one of its buff states. Wounded targets are healed, others are blessed. [exe/inf]
- The reserve (row 3) can never be targeted.

### 2.4 Physical damage [exe; "attack minus defence" is doc]

For attack kind `K` (melee, flank-melee or shot):

```
atk = attacker.Attack(K) + attacker.bonusAttack            // AttackBlow or AttackShot
      (Splash bonus: ×0.8 on the main target, ×0.4 on neighbours)
def = target.Defence(K) + target.bonusDefence + target.buildingDefence
      (+ Row2Def if K = shot and the target is in row 2)
if SpearDefense and battle turn 1 and K is melee:  def ×= 3
if flank strike:  def /= 2; if attacker has FlankStrike: atk ×= 2
if attacker has ArmorIgnore / PoisonArmorIgnore / VampirsGist / OldVampirsGist:  def = 0
dmg = atk − def, at least 1                            // no random roll
dmg ×= 2/3 if target has VampirsGist, OldVampirsGist or Evasive
dmg ×= 2/3 again if target has Garrison and building defence ≥ 10
dmg ×= 3/10 if K = shot and target is Dead or FastDead
dmg ×= 90/100 if the target's army has a Knight hero
dmg  = 1 if target is Unvulnerabe or Ghost
dmg += 10 (GodAnger) or 20 (GodStrike) for the attacker's bonus
dmg  = max(1, dmg · (100 − target.Evasion)/100)       // Community
```

- There is **no random damage range**: damage is deterministic.
- An alternative percentage formula, `atk × (1 − def/100)`, exists in the code, but real
  battles set the subtractive mode. The other mode is probably used for estimates. [exe]
- Counterattacks happen only through bonuses (Counterblow, PreventiveStrike). [doc]
- The AI estimates "hits needed to kill" as `ceil(targetHP / dmg)`. `CrazyAI` (random
  scatter in target choice) is read from the ini but **never used** by this build. [exe]

### 2.5 Death and the hero

- A unit at 0 HP is dead. It stays in the army as a corpse until it is resurrected (in a town
  or church, within 7 days) or buried. Dead units cannot hold items. [doc]
- The hero cannot die while some unit of the army survives. After the battle the hero turns
  out to be alive but badly wounded (an event condition checks "the hero has only 1 HP"). If
  the whole army dies, the scenario is lost. [doc]
- An AI feudal lord cannot be killed for good while he still owns a building: he retreats
  there and recovers. To finish him, capture all his buildings. [doc]
- AI armies can respawn after an editor-set number of days, either the leader alone or the whole army. [doc]

### 2.6 Rewards

- The winner takes the loser's gold divided by `VictoryGoldDiv` (2), and at least
  `MinVictoryGold` (25), or everything if the loser has less. [doc]
- The loser's items become trophies. Surrendered units give mana. Capturing a castle or fort
  changes its owner. Clearing ruins gives the treasure. [doc]
- Editor flag: an army can "carry no personal money", and then there is no gold to take. [doc]

---

## 3. Magic

### 3.1 Resources and who casts

- The two resources are **gold** and **mana** ("магическая энергия"). Both belong to the hero
  and cannot be dropped or stored elsewhere. Mana comes from villages (peasants pray at
  midnight), from surrendered enemies, from events and from the witch option in villages. [doc]
- **Global spells** are cast by the hero from the spell book on the world map, before a
  battle, onto the whole own army (`Target=Hero`) or a whole enemy army (`Target=Enemy`).
  They cannot be cast during battle. **Casting costs game time**, so the target may run off
  or reach you first. [doc]
- **Battle magic** comes from mage units using their own `MagicPower`, not from the hero's mana. [doc/exe]
- The spell book has a capacity limit (message "no room in the book"); the size is [unk].
  Spells are **learned for gold** at a sanctuary or library in a town, church or altar, and
  **cast for mana**. [doc]
- The Archmage casts twice as fast for half the mana. The Caster bonus gives −20% time and cost. [doc]

### 3.2 `Rus_Spells.ini` fields (34 spells + template)

| Key | Meaning |
|---|---|
| `Name` | Spell name. |
| `CostGold` | Price to learn it. |
| `CostMana` | Mana cost to cast. |
| `Type` | School: `Life` / `Elemental` / `Death`. |
| `TimeWork` | Duration in game hours at caster level 0. Empty means instant. 9999 in practice means permanent. It may scale with the caster's level [unk]. |
| `TimeCast` | Casting time in game hours at caster level 0. |
| `Target` | `Hero` (own army) or `Enemy`. The exe also knows `OneEnemy`. |
| `Icon1..3`, `ColorC1..3` | Three layered icon images plus a colour tint for each. |
| `Effect1..3` | Visual effects (bottom, back, front): `file,r,g,b,duration_ms,yOffset,scale×1000,start_ms`. |
| `DeltaFixedHits` | Instant HP change: + heals, − damages (for example Lightning −15, Chain lightning −40). |
| `DeltaPercentHits` | Instant % HP change. Recognised by the exe, unused in vanilla. |
| `d-<Stat>` | Flat change for the duration. |
| `p-<Stat>` | Percent change for the duration. |
| `p-LifeLose` | Percent life drain ("отбирает жизнь"), used by scripted curses. |

At most three modifiers fit on the spell screen. Stats that can be modified: Hits, AttackBlow,
DefenceBlow, AttackShot, DefenceShot, MagicPower, Initiative, Manevres, Protect*, Regen,
Vampirizm. Several spells (cost 0 or 1) are **scenario-only** (Ghost gift, Gate guard, Rockfall,
Armageddon, Transfer ritual …) and are applied by events. [doc/inf]

Village option 1 ("priest casts a very long-lasting good spell instead of tribute") gives
spells beyond their normal duration. [doc]

### 3.3 Battle magic by school [exe]

The caster's power `P = MagicPower` (with bonuses). For **hostile** effects,
`P = round(P × (1 − Protect_school(target)/100))`. Friendly effects are not reduced.

Actions: **strike** (damage), **curse** (debuff), **heal**, **bless** (buff).

| | Life | Elemental | Death |
|---|---|---|---|
| Strike dmg | P; **×2 vs Undead**; ×0.75 vs Elemental | 0.75·P | P; ×0.5 vs Undead; ×0.75 vs Elemental |
| Heal | +P; none on Undead or Elemental | +P/2 | works **only on Undead** |
| Bless | def +(3P/(2·BlessMainSpell)+1) = P/4+1; atk +3P/(2·BlessNextSpell) = P/8. None on Undead or Elemental. | +actions f(P); initiative +(P/WizardMainSpell+1) = P/7+1 | atk +(P/BlessMainSpell+1) = P/6+1; def +P/BlessNextSpell = P/12 |
| Curse | def −(1+P/(⅔·CurseMainSpell)) = −(1+P/3); atk −P/10 | actions −f(P); initiative −(1+P/7) | atk −(1+P/CurseMainSpell) = −(1+P/5); def −P/CurseNextSpell = −P/10 |

- `f(P)` = 0 if P<20, 1 if P<45, 2 if P<100, 3 if P≥100. That is how the Archmage "freezes" enemies.
- A curse cannot push remaining actions below 0.
- Heals are capped at max HP. Damage is capped at current HP.
- The attack buff is dropped on targets that have no attack.
- GodAnger/GodStrike add +10/+20 to magic strikes as well.
- Which hostile action a mage uses is [inf]: the preview code picks *strike* for Life casters and *curse* otherwise.
- **Magic power drains in battle** by `DecSpellLife/Death/Elemental` (2/2/5) per turn, down
  to a floor of `MinSpellLife/Death/Elemental` (15/0/15). Community settings can override
  this per unit. The help's "the archmage loses 5 power per turn" matches. [doc/exe]
- Buffs and curses last a limited time. The Community bonus EternalGift makes them last the whole battle. The exact duration is [unk].

---

## 4. Artifacts and items (`Rus_Artefacts.ini`, 167 entries)

| Key | Meaning |
|---|---|
| `GlobalIndex`, `Name`, `Descript` | ID, name, text. |
| `Icon` | Image file, for example `A000.Tga`. |
| `Cost` | Base price. A **negative price marks a personal item** that cannot be taken away (family relic, −1700). [inf] |
| `Type` | `BlowWeapon` (warriors only), `ShotWeapon` (shooters only), `Staff` (mages only), `Armor`, `Helm`, `Shield`, `Ring`, `Amulet`, `Potion`, `Item` (trade goods, cannot be equipped). [doc] |
| `Bonus` | Grants a unit bonus from 1.3 while worn. |
| `Magic` | Grants or changes the magic school (for example Death staff → DeathMagic). |
| `d-<Stat>` | Flat addition. |
| `p-<Stat>` | Percent change. |
| `f-<Stat>` | **Fixed value**. On gear it sets the stat to that value (armour `f-DefenceBlow=26`, sword `f-AttackBlow=55`, `f-Manevres=2`). On potions it is the healing amount (`f-Hits=75`). Application order with d-/p- is [unk]. [inf] |

Rules [doc]:
- A unit wears **4 items**. The editor note says "the leader puts on all 4 artifacts at once".
- It holds **only one weapon** and **no two items of the same type**.
- Class limits: warrior weapons, shooter weapons, mage staffs, and "dark forces only" items.
- The dead cannot hold items.
- Potions are dropped onto a unit's card and are not kept in the inventory. Their effect,
  except healing, lasts until the end of the **next battle**.
- The hero has a separate backpack inventory; its size is [unk].
- Selling pays **`ItemSaleCost`=25%** of the price ("4× less than buying"). Merchant: +50% on sale, −30% on purchase.
- Prices differ between buildings, based on the building's relation to the player (a bad relation raises prices).
- Markets have fixed stock plus random items within an editor price range. Churches usually have a max price of 245, so they sell potions and church amulets.

---

## 5. World, economy, time

### 5.1 Time [doc]

- Game time runs **only while the army moves, waits or casts a spell**. Standing still freezes time.
- AI armies share the same clock and move at the same time as you.
- The clock uses minutes (events store absolute minutes since year 0) and shows day, month and year.
- Wait buttons: 1 hour, or 4 hours (the 4-hour wait cannot be cut short).
- Villages refill **at 00:00**. The daily report (income, wages, unpaid units) and the autosave come at **12:00**, as
  the gameplay footage shows (video notes, §1).
- The calendar shows year, month (1–12) and day (**0–29**) and whole hours: `1204 год, 5 месяц, 19 день, 9 час`.
- The path preview shows the travel time.
- Community keys: F1/F2 quick load, F3 save, F4/F5 endless time skip.

### 5.2 Movement and map [doc/unk]

- Terrain textures (editor order): shallows/fords, coastal water, deep sea, lava fields, road,
  grass lowland, grass plain, dry plain, swampy ground, impassable bog, sand dunes, clay,
  stony ground, scorched land, snow, impassable snowdrifts. The exe enum is Shallow, Water,
  DeepWater, FlameLand, Road, Toto, LowLand, Land, Plain, Swamp, DeepSwamp, Desert,
  Badground, Rock, Dust, Snow, Ice.
- Mountains and rocks block. Hills slow. Normal trees slow. Large trees ("непроходимые чащи") block. Deep water blocks on foot.
- The **per-terrain movement cost is not in the data files** and was not found in the exe [unk].
- Fog of war: unexplored ground is dark and **counts as impassable** until the hero has seen it.
  Scripted "lanterns" reveal areas (radius up to 24, at most 5 events per point). [doc]
- Army speed: Ranger +20%. The editor sets a per-army "speed correction" (about −3..+5).
  Community events can change speeds.
- Grid: plain 32×22 px cells, 8 neighbours (evidence in `dtm-format.md` §4) [inf].
- Contact: you meet another army when it is on an adjacent cell. Click it to talk or fight.
  Hostile armies attack you as you pass by. [doc]
- Ships: rented at a **shipyard for `ShipCost`=250 gold**. The ship takes you anywhere on the
  coast and waits where you land. Pirate and merchant AI ships exist. [doc]

### 5.3 Buildings (editor types) [doc]

| Type | What it does |
|---|---|
| Town | Quests and rumours, market, hiring, healing and resurrection, spell library. Some income. Its garrison is AI-only. |
| Village | Tribute: gold (and mana) at midnight, accumulating up to an editor maximum. Can pay the owner a daily income (rarely used). Barracks AI-only. Instead of tribute it may offer one of 5 options: a long-lasting blessing, healing the army, furs worth more than the tribute, a magic power ritual, or paying off unpaid units. |
| Castle | Must be captured. Hire from the garrison, heal, leave troops. Small steady gold income. Can have extra garrison defence. Quests. A friendly castle only lets you hire from its garrison and heal. |
| Fort | A small castle. Rogue armies whose home it was may suddenly retake it. |
| Church | Learn spells, hire monks, buy potions and anti-evil amulets, heal, **resurrect**. |
| Tavern | News and quests. |
| Market | Buy and sell items. News and quests. |
| Smithy | No function (news and quests only). |
| Shipyard | Rent a ship. |
| Altar | AI hiring. May have a shop or library. |
| Entrance (dungeon) | No function. |
| Ruins | Garrison plus treasure. The defenders use the treasure items against you. Some ruins are hidden under mountains or forest and show grey on the minimap. |
| Bridges (stone and wooden), obelisk | No function. |

Building screens: main hall (description and quests), barracks (hire and heal), garrison
(leave troops; named units cannot be left), shop, sanctuary (spells). You enter a building
by stepping onto its cell. Only the building the hero stands in can be managed. [doc]

Garrison fights run automatically while you are away. Troops in buildings get a defence bonus. [doc]

Hiring stock: each building lists up to 6 unit types, each with a start count and a max. Stock
regrows over time (`MaxDayCountForNewUnit=10` days). The "all types" flag lets the player hire
any listed type. [doc/inf]

### 5.4 Income

- Castles (and optionally towns and villages) have an editor-set daily gold income. Villages
  have gold and mana income with a maximum stock. [doc]
- The hero screen shows the daily income and the daily wages. [doc]
- The editor can give an AI army extra gold income. [doc]

### 5.5 Factions and relations [doc]

- Four groups: player (green), ally, neighbour, enemy. The editor also uses green, blue, yellow and red.
- Each group has a relation to every other group, from −3 to +3 (0–100%). Relations drive AI hostility and prices.
- Community events can change an army's or building's group and relations at runtime.

### 5.6 AI (`_Global.ini`) [doc]

- Behaviour styles: **feudal** (like the player: income, tribute, wages, hiring, shopping),
  **rogue** (no wages, no tribute, hires rogue units, retakes its forts), **peasant** (wanders).
- Target models: standard, aggressive, passive, hoarding, trading. The `Min/Max…Target` lists
  hold one priority per model for attacking, random wandering, "talking" to friendly armies,
  healing, garrisons, shopping and villages.
- Other settings: `AIDistance0..2` view ranges, `ZeroDensity`, `NeedUpkeepDay=5` (the AI keeps
  5 days of wages in reserve), aggression, patrol radius, and flags (ignored by AI, chases only
  the player, no random targets, no talking, no interest in buildings).
- **Ini bug**: `_Global.ini` spells the key `MixHealingTarget`, but the exe reads
  `MinHealingTarget`, so that line has no effect. [exe]
- `[AIArmyGeneration]` lists the unit IDs the generator may use per army theme (Normal, HolyArmy, Piesant, Rogue, Assasin, Undead, Hero, Vampires).
- `ShotWeaponRange=60`: shooters with ranged attack of 60 or more count as cannon ("пушкарь"), probably for visuals and sound [inf].

---

## 6. Scenario scripting (editor manual + Community guide)

- **Engine loop** [doc]: on every tick, check all events; run the first whose conditions
  hold; repeat until none fires. So one event can enable another within the same tick.
- **Event types**: global (checked anywhere), local (checked when the player is at a given
  map cell or building), quest (goes into the journal), rumour (a local event the player must pick).
- **Time window**: start date and hour, how long it stays active, and a repeat every N days.
  Flags: *subordinate* (runs only when another event triggers it), *relative* (no own start
  time; another event sets it), *repeatable*.
- **Conditions**:
  - hero class
  - events that happened with answer Yes, happened with answer No, or have not happened
  - flag variable set or unset (`/name` means unset)
  - a yes/no confirmation question (optionally repeatable after Yes)
  - the player beat certain armies
  - meeting a given army (needed to talk to friendly armies instead of fighting)
  - hero stats: level, gold, "holiness and mana", number of units, army strength
  - who owns buildings, artifacts, or named units (player, green, blue, yellow, red, or "not the player")
  - army beaten by anyone, army active or inactive, army at its home building
  - Community: a named unit's class, active spells on armies, army position
- **Results**:
  - text (no text means a silent event), a picture, a chained subordinate event, closing a quest
  - resources: XP, gold, mana
  - moving a relative event's start time, forcing a delay on the player, setting or clearing flags (`+name`, `-name`)
  - adding units (optionally taken from another army), removing units ("added unit" / "any unit"; last joined leaves first, optionally sent to another army or moved next to the hero)
  - learning spells, giving or taking items
  - revealing map areas (lanterns), showing, activating or deactivating armies, changing patrol radius
  - changing the hero's class (class bonuses are lost), starting a battle with an army
  - "no meeting with army" (clears the meeting flag, preventing loops), applying a spell to the player's army
- **Community extensions**: the "no meeting" flag plus a patrol-change value selects an opcode,
  and the resource fields carry its arguments:
  - 1–5: edit or compare fields of other events (relative offset)
  - 6: equip items on an AI unit (or a building's garrison with a negative ID)
  - 7: replace a unit type in a slot
  - 8: set army speed
  - 9: change faction
  - 10: change relation
  - 11: apply permanent spells to a unit or army
  - 12: place a named unit in a slot
  - 13: give XP
  - 14: check spells
  - 15: campaign branch (map number and variant)
  - 16: remove spells from the book
  - 17: change the army's map model
  - 18: random flag RAND 1..9
  - 19: set an AI target or check an AI position
  - 20: teleport the player
- **Scenario settings**:
  - name and description; the **victory event** and **defeat event**
  - hero start: XP, mana, gold, start building (optionally per class), items, spells, army
  - relations between groups
  - campaign: first map or next map, the next scenario's name, and what carries over from the previous map
- **Map file** (`Maps_Rus/*.DTm`, binary): the ID notebooks record record sizes: army 89 bytes, building 358 bytes, lantern or event point 99 bytes, event about 165 bytes plus text. See the notebooks for field offsets if a map loader is needed.

---

## 7. Prototype guess (before Stage 2) → real rule

Stage 2 replaced the left column with the right one for units, battle, items and wages;
see section 8 for the choices Razdor makes where the original is unknown.

| Topic | Razdor now | Original |
|---|---|---|
| Formation | 2 rows × 6 | **3 rows × 4** (front, back, reserve). 6 columns only with the Community "wide row" option. 12 units max. |
| Reserve | none | Row 3: cannot attack or be attacked. Used to rotate wounded units out. |
| Damage | random min..max roll, minus armour, min 1 | **Fixed** `Attack − Defence`, min 1. Separate melee and ranged defence. No roll. |
| Armour / defence | one "armor" value | `DefenceBlow` and `DefenceShot` separately. Back row gets +5 **vs shots only**. |
| Flank | diagonal with empty straight-ahead cell → ×2 | Diagonals (c±1) are normal hits. The "flank" is a long strike to the nearest front-row enemy when **all three** opposite cells are empty. It **halves the target's defence**. ×2 attack only with the FlankStrike bonus. |
| Shooters | may target anyone | From the back row: anyone in the enemy front or back row. From the front row: only adjacent enemies, unless none are adjacent. Never the reserve. |
| Magic | ignores armour | `MagicPower × (1 − Protect%/100)` by school, with nature multipliers (Life ×2 vs undead). Buffs, curses, heals and strikes follow the table in 3.3. Mage power drains each turn. |
| Healer | heal 10, range 4 | Heals by its magic power (Life: `P`, not on undead). Can also bless. |
| Counterattack | none | None by default. The Counterblow and PreventiveStrike bonuses add one. |
| Turn order | initiative desc, ties → player | Initiative desc. **Attacker +1 initiative**. |
| Actions | Ranger 2, others 1 | `Manevres` 1–3 per unit (most shooters and mages 2). Ranger hero 2, Archmage 2, Knight 1. |
| Battle cap | 20 rounds, stalemate | `BattleEndTurn` = **25**. No retreat option. |
| Hero stats | Knight 60 HP 10–14 dmg armour 5 | Knight 80 HP, atk 45, def 15/10, init 9, 1 action. Archmage 50 HP, magic 25, prot 35%, init 26, 2 actions. Ranger 65 HP, shot 30, def 5/5, init 19, 2 actions, regen 5. |
| Hero bonuses | none | Knight: army −10% physical damage taken. Archmage: spells 2× faster and ½ mana. Ranger: +20% map speed, +20% HP/day heal. |
| Hero death | hero dead = defeat | The hero survives (wounded) while any unit lives. Defeat means the whole army is wiped. |
| XP / levels | out of scope | Core system: XP to next level = `StartExp·(LevelMult/100)^L`, `d-*` stat gains, upgrade trees. |
| Wages | fixed per type (3–5) | Derived from Cost (see 1.5). Unpaid units skip battles. Garrisoned units stop costing pay after 1 day. |
| Castle heal | free on entry | Costs gold (~50% of Cost × wounded fraction) and 60 min. Resurrection 3× Cost, town or church, ≤7 days. |
| Church | free full heal | Paid healing, resurrection, spells, potions, monks. |
| Village | tribute 10 gold **or** priest heal | Tribute of gold **and mana** at 00:00, up to an editor max. Priest, hunter, witch and innkeeper alternatives. |
| Items | 4 slots, any unit, 1 of each type, pack 16 | 4 slots and 1 of each type are right. Weapon and staff class limits. Only one weapon. Potions used from the army screen (effect lasts until the next battle ends). `f-` fixed stats. |
| Sell price | 50% | **25%** (`ItemSaleCost`). Merchant +50% sell, −30% buy. Prices depend on faction relation. |
| Market restock | weekly Monday | Fixed stock plus random items in a price range. The restock rule is [unk]. |
| Loot | random items | The loser's gold ÷2 (min 25), all its items, mana from surrendered units. Ruins: an editor treasure. |
| Time | moves only when walking | Also passes when **waiting** (1 or 4 h) and when **casting global spells**. |
| Day tick | midnight | Villages refill at midnight; the daily report (income, wages) is at noon (footage). |
| Terrain cost | road 30 min … swamp 3 h | Not in the data [unk]. 16 textures plus hills and trees (which slow), mountains and thickets (which block). |
| Ships | out of scope | Shipyard rent 250 gold, any coast. |
| Fog of war | out of scope | Unexplored ground is impassable until seen. |
| Global magic | none | Spell book: learn for gold, cast for mana and time, on a whole army, before battles. |

---

## 8. Razdor implementation choices

Where the sections above say [unk] or [inf], Razdor (`src/rules/`) makes these choices.
Each is marked *(guess)* in the code.

- **Levels** start at 1 as hired; XP to the next level is `StartExpirience ×
  (LevelMultipler/100)^(level−1)`. Promotion starts the new class at level 1 with no XP and
  the same HP fraction; items the new class cannot wear go to the pack.
- **Item modifiers**: every worn item's `f-` sets its stat first, then all `d-` are added,
  then all `p-` are summed and applied. An item's `Magic` replaces the unit's school.
  Potions: `f-Hits` heals at once; other modifiers last until the next battle ends.
- **Tactical cost** (for XP): `Cost × CostMultipler/100`, +10% per level above 1.
- **XP pool**: as in 1.4 without the damage-exchange term; weight per survivor
  `(4 − row)·10 + damage dealt + HP healed`; at least 1 each.
- **Wages**: see the Stage 4 notes below for the two hiring kinds. Medic 15% and Ranger
  20% daily healing do not add up (the larger applies).
- **Battle turn limit**: `BattleEndTurn` full turns are played, then a stalemate; nobody
  wins, the player withdraws.
- **Hero**: at 0 HP he leaves the field like any unit; if anyone of his army survives he
  returns with 1 HP after the battle.
- **Effects**: blessings and curses last 3 turns including the one they are cast in. A new
  one replaces the old; a unit holds at most one blessing and one curse. Friendly mages do
  not bless an already blessed unit.
- **Magic**: a strike needs power left after protection; a school with no
  `MagicDirection` counts as `ToAll`. Default hostile action: strike for Life casters,
  curse otherwise (the right click picks the other).
- **Regen** heals `Regen`% of max HP at the start of every turn from turn 2. **Poison**:
  a physical hit of more than 1 damage poisons; 15% of max HP per turn, can kill.
- **Counterblow**: one melee strike back after being struck in melee (not after shots or
  magic), by warriors only.
- **Piercing** (`ArmorIgnore`, the vampire gifts, `Artillery`): the unit's defence is
  ignored, building defence still counts.
- **Garrison**: in a building with defence ≥ 10, attack and defence ×2 at the start and
  damage taken ×2/3.
- **Movement into the reserve**: any empty reserve cell, from the back row only.
- **Collapse**: when a front row is empty, the back row steps forward; if the back row is
  empty too, the reserve does. It also happens after moves.
- **AI**: heal an ally below half HP; else a killing blow on the most dangerous target;
  else (non-Life casters) curse the most dangerous uncursed enemy; else the attack needing
  the fewest hits; else heal or bless. Units without a target step towards a cell with one
  (warriors to the front row, others to the back row); units in the reserve stay there.
- **Community bonuses not implemented** (no effect yet): Hunger, Berserk, Exhaustion,
  Drying, CtrPoison, Suicide, Caster, Splash, Fortify, Dominate, Concentration, PoisonS,
  Stun, Potent, FirstShot, Bastion, Flying, Flock, Bleed, HoldLine, ArmorBreaker,
  FasterAttack, NoHeal, PreventiveStrike, Neutralize, KillingStrike, BloodThrist, Assault,
  EternalGift, FateGift. `PoisonArmorIgnore` counts as piercing; `Evasion`,
  `MinMagicPower` and `ManaDrain` are implemented.
- **World grid** (Stage 3): scenarios use 8-neighbour rectangular cells (32×22 px, a
  vertical step is 22/32 of a horizontal one, a diagonal step √(32²+22²)/32); travel time is
  the cell's cost times the length of the step. The built-in demo keeps its own hex grid
  (odd rows shifted right).
- **Terrain costs** (minutes per cell, on foot): road 30; grass lowland, grass plain, dry
  plain 60; clay, stony soil, scorched land 75; sand 90; marsh, shallows/fords, lava fields,
  snowy ground 120; coastal water, deep sea, impassable swamp, impassable snowdrifts block.
- **Objects**: mountains (classes 5, 6), dense thickets (11) and rocks (8) block; hills (1–4)
  and trees (9, 10) add 50%. A massif (hills, mountains, rocks) of sprite family `f` (tens
  digit of the sprite id) covers a square of radius `(f − 1) / 2` whose bottom row is its own
  cell. Road cells are never blocked by objects (a few dozen thickets stand on roads).
- **Buildings**: the footprint (`size_x × size_y`, up and left of the anchor) blocks, except
  the **entry cell**: the footprint cell next to the largest open region (judged with all
  walls up), then one a road arrives at, then the nearest to the bottom-row middle. Stepping
  onto the entry enters the building; a hostile castle, fort or ruins with a garrison stops
  the hero there and the garrison fights (its extra defence counts). Winning takes a castle
  or fort (owner = player, faction 1, its income and mana count from the next noon) and
  gives ruins' treasure gold and items. Bridge footprints are road.
- **Villages** start with one day's tribute; at midnight it grows by `gold/mana per day` up to
  the maximum. Collecting takes all of it.
- **Armies**: model 7 / byte 63 = off the map at start; ships are not simulated yet. Hostile =
  the army's own attitude towards the player < 0. Hostile armies chase the hero within 6
  cells and fight on contact (neighbouring cell); others greet once and let him pass.
  Patrolling armies wander within their patrol radius, resting 30–180 min between legs.
  Speed correction: ±10% per point. Troops: the middle byte of each triple is levels above
  the first; the leader is a troop of its own. Loot: `gold income / VictoryGoldDiv`, at least
  `MinVictoryGold`, plus the army's artifacts.
- **Hero start**: the preset's cell (moved to its building's entry if it lies in the walls),
  gold, combat experience, troops and artifacts. Mana starts at 0.

Stage 4 (buildings and economy, `rules/town.rs`, `rules/game.rs`):

- **Building tabs**: every building has a main hall; towns, castles, forts and churches a
  barracks (villages and altars hire for the AI only); the player's castles and forts a
  garrison; a building with goods a market, with spells a sanctuary; villages a tribute
  tab. A building of attitude below 0 still trades and heals (the footage shows trading at
  a market of attitude −2) but does not hire or pay tribute. A garrison still to be beaten
  opens no window. An ill-disposed castle or fort with no garrison is taken by walking in.
- **Main hall**: lists the building's quest and rumour events (`World::local_events`);
  local events fire by themselves and are not listed. Running them is the event engine's.
- **Wage kinds**: `Nature=Rogue` units are kind 2 (mercenary: `Cost / CostMercenaryDiv`),
  everyone else kind 1. The kind is kept per unit. Elementals are hired, healed, raised and
  paid in mana (Community). Corpses are not paid. When gold runs short the squad is paid in
  order and the rest are unpaid; paid again at the next noon with enough gold.
- **Desertion**: a unit unpaid at `MaxTimeNotUpkeep / 1440` noons in a row (7) leaves; its
  items go to the pack. The village innkeeper option clears the unpaid state.
- **Garrison**: a unit left there is paid at the first noon after it was left, then never;
  it heals `GarrisonAutoHeal`% of max HP at every noon. Scenario garrisons of buildings the
  player owns at the start become his (already past their paid day). Capacity: the
  formation's.
- **Barracks regrowth**: at every midnight a type below its maximum gains `max` progress;
  every `MaxDayCountForNewUnit` progress is one unit, so an empty barracks is full again
  after `MaxDayCountForNewUnit` days (5 militia: one every 2 days). The editor's
  "all types" flag is read but has no effect.
- **Healing**: `ceil(Cost × HealingConst% × missing / max)` for a full heal of one unit,
  `HealingTime` minutes of game time per unit. No free healing on arrival.
- **Corpses and resurrection**: the dead stay in the army (HP 0, still in their cell) and
  drop their items into the pack (what does not fit is lost). Raising costs
  `Cost × ResurectConst%`, takes `HealingTime`, restores full HP, and is possible until
  `MaxTimeResurection` minutes after death; later the body is buried automatically. The
  army screen can bury (dismiss) a corpse.
- **Market prices**: markup `max(0, 1 − attitude) × 15%` on the base price, then the
  Merchant's −30%. Fitted to the footage: with a trader hero, two markets of attitude 1
  sell at exactly 70% of the base price, one of attitude −2 at 70% × 1.45 (120 → 122,
  200 → 203, 210 → 213). Selling pays `ItemSaleCost`% (+50% with a Merchant), whatever the
  relation. The footage's spell prices do not match `CostGold` of the Community data (some
  are twice it), probably a data difference between versions; Razdor charges `CostGold`.
- **Market stock**: the fixed goods plus `random` different market items whose base price
  lies in `[min, max]`. Fixed goods once bought are gone for good; every 7 days the random
  part is drawn anew (demo and scenarios alike).
- **Backpack**: 40 items (the original's inventory is a scrolling 5-wide grid with more
  than 25 items in the footage). **Spell book**: 15 spells (its window has 3 × 5 cells).
- **Victory loot**: an army pays `gold / VictoryGoldDiv`, at least `MinVictoryGold`, or all
  of it when it has less, plus its items. A captured castle or fort pays one day of its
  gold income (the footage: +30 for a fort of income 30, +125 for a castle of income 125).
  Every beaten enemy unit gives its `Surrender` value in mana (the footage: +20 mana from a
  fort garrison with one unit of `Surrender=20`).
- **Village alternatives**: collect the tribute, or instead the priest heals the army, or
  the innkeeper pays off the unpaid. The long blessing, furs and magic ritual are not in
  yet (TODO, with spells).
- **Not used yet**: `CostGoldDiv`, the Archmage's world-spell bonus, "dark forces only"
  items, ships at shipyards, named units that cannot be left in a garrison.

## Appendix: `_Global.ini` `[GlobalOptions]` quick reference

| Key | Value | Use |
|---|---|---|
| WizardMainSpell | 7 | Elemental bless/curse initiative divisor. |
| BlessMainSpell / BlessNextSpell | 6 / 12 | Life and Death bless divisors. |
| CurseMainSpell / CurseNextSpell | 5 / 10 | Death curse divisors. Life curse uses ⅔·CurseMain and a fixed 10. |
| DecSpell* / MinSpell* | 2,2,5 / 15,0,15 | Per-turn magic power loss and floor (Life, Death, Elemental). |
| CrazyAI | 0 | Read but **unused**. |
| Row2Def | 5 | Back-row bonus vs shots. |
| BattleEndTurn | 25 | Turn limit. |
| HealingConst / HealingTime | 50% / 60 min | Heal cost and heal time. |
| ResurectConst / MaxTimeResurection | 300% / 10080 min | Resurrection cost and time window. |
| NeedUpkeepDay / MaxTimeNotUpkeep | 5 days / 10080 min | AI gold reserve / how long an army goes unpaid. |
| VictoryGoldDiv / MinVictoryGold | 2 / 25 | Gold loot. |
| CostRecrutDiv / CostMercenaryDiv | 2 / 2 | Wage divisors. |
| MaxDayCountForNewUnit | 10 | Barracks restock speed. |
| GarrisonAutoHeal | 10 | % HP healed per day in a garrison. |
| ShotWeaponRange | 60 | Threshold for cannon-type shooters. |
| MainExpCorrection / ExpCorrection | 30 / 50 | XP pool %, and strength-ratio skew. |
| AIExpiriencePercent / HeroExpirienceModificator | 100 / 50 | XP scaling for AI / player. |
| ItemSaleCost | 25 | Item sell %. |
| `[Costs] ShipCost` | 250 | Ship rent. |
