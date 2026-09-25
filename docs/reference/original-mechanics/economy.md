# Discord Times: economy, buildings, spells, scripting (from the Community exe)

Source: the user's `DiscordTimes.exe` (Community Update, Unstable), static reading only.
The rules are written in my own words, and addresses are given so they can be checked.
Confidence levels: **H** = code read end to end, **M** = the main path was read but
edge cases were not, **L** = inferred.

Conventions found in the exe that the rules below rely on:
- Delphi `Round` (0x402dd0) rounds halves to even, so 2.5 → 2 and 3.5 → 4. Integer `/` truncates towards 0.
- Game time is kept in 1/100 minutes (`0x68dcb8`). "now" means that value /100 plus an offset (`0x68dcbc`).
- Unit HP −1 means "unhurt" and 0 means dead. A positive HP is a wounded unit.
- **Difficulty factor F** (0x4b8a10): `OptValue10` ("impossible difficulty") = 1 gives F = 100, and
  anything else gives F = 120. The user's ini has it on, so F = 100 there. F makes heal and
  resurrection prices cheaper (÷F/100), item sales dearer (×F/100) and the player's castle
  income bigger (×F/100). It also scales the player's battle XP (experience.md §3); Razdor
  models F only there.
- Unit natures (0x48f3de): 0 ordinary, 1 Undead, 2 Elemental, 3 Rogue, 4 Animal.
- Item types (0x4990cc): 0 BlowWeapon, 1 ShotWeapon, 2 Armor, 3 Helm, 4 Shield, 5 Staff, 6 Amulet, 7 Ring,
  8 Potion (also the default), 9 Item.
- The hero's unit type is kept 0-based at `0x68dccc` (0 Knight, 1 Archmage, 2 Ranger).

---

## 1. Wages

**Hiring kind (who is kind 1 or 2)**: H. AddUnit (0x495ce0) stores a kind per unit (unit+0x19d).
- **Kind 1 (recruit)** covers everything the player hires: from a barracks (0x4b0fab), a garrison
  (0x4b0ff5), the starting army and garrisons at load (0x4b44db, 0x4b52ed), and AI troops at load.
- **Kind 2 (mercenary)** appears only when an AI army hires in a building it does not own
  (0x4a6ac5: building owner ≠ the hiring army). In its own building an AI hires kind 1.
- **Kind 0** is the hero and the AI army leaders (0x4b4383, 0x4b46ae). Kind 0 is never paid.
- **Kind 3** is a unit added by an event (0x4a94b3). Kind 3 is never paid either.
- The kind does **not** depend on `Nature` (Razdor's guess of Rogue → 2 is wrong).

**Wage formulas** (0x4a163c, 0x4a1857): H. C is the unit type's `Cost`.
- Kind 1: `Round(C / CostRecrutDiv × f)`, where f = 0.25 if C ≤ 50, 0.5 if C ≤ 100, 0.75 if C ≤ 150, and 1 otherwise.
- Kind 2: `C div CostMercenaryDiv`, as an integer.
- Every living unit is billed, and so is every corpse (the loop does not skip HP 0).

**Noon payment** (0x4a41d8, called at noon for the player at 0x4ac33d and for each AI army at 0x4a5534): H.
1. Next check = the next day's 12:00.
2. Daily income = the army's own base income. The engine then adds the gold stock of each castle
   or fort the army owns (×F/100 for the player), and the gold stock of each village whose linked
   building (byte 293) the army owns. Those stocks are reset to 0. See §3.
3. Gold += income.
4. If the army pays wages (army+0x16b7 = 0), all wages are deducted at once.
5. If gold is now negative, every unit is marked unpaid. The engine then gives back the wage of the
   **cheapest** unit that is still unpaid, marks it unpaid, and repeats while gold < 0. So the most
   expensive units keep being paid. Afterwards **gold is set to 0** (any remainder is lost).
   Paid units get "last paid = now".
6. On such a short day, every unit whose `last paid + MaxTimeNotUpkeep < now` **leaves the army**
   (0x4a4608 → remove 0x4965b0). With 10080 that is 7 days without pay. The check runs only on
   noons when money is short.
7. With enough gold, every unit is marked paid, with last paid = now.
- Unpaid units (flag unit+0x1a5 = 0) do not fight. That part is doc + flag, and the battle code belongs to the battle agent.
- **Garrisons are never paid**: H/M. A garrison is the building's own army, and the noon routine runs only for the
  player and the AI armies. So "pay stops on the second day" really means garrison units pay nothing at all.

**Community: elementals and Rear Service**: M.
- A kind-1 unit of Nature Elemental is paid its wage in **mana** (0xc25e97, 0xc25eef). A kind-2
  elemental still pays gold.
- If mana would go below 0, it is set to 0 and every elemental is marked unpaid (0xc25f7d, 0xc25feb).
  Their "last paid" is still updated on days when gold covered everyone else.
- **AddPayment (Rear Service, bonus 16)**: if any unit of the *player's* army has bonus 16 (0xc2503b),
  the gold wage bill is multiplied by 178/256 ≈ 0.695 (−30%). This happens only when the player's
  daily income is not 0. When the player's income **is** 0, the multiplier is 78/256 ≈ 0.305
  (0xc250e4 / 0xc25001). This looks like a Community quirk, so reproduce it as written. The check reads
  the player's army even while an AI army is being paid.

## 2. Prices, market, barracks, healing, resurrection

**Relation price factor** (0x4a03ec), used for market purchases and for AI hiring: H.
- Take the building's attitude (−3..3) towards the buyer's faction, or +3 if the buyer owns the building.
- Price = `Round(base × m)`, where m depends on the attitude:

  | Attitude | −3 | −2 | −1 | 0 | +1 | +2 | +3 or own |
  |---|---|---|---|---|---|---|---|
  | m | 1.7 | 1.45 | 1.25 | 1.1 | 1.0 | 0.9 | 0.75 |

- **Merchant** (bonus 5 on any unit of the player's army, 0x4bc26b): the purchase price loses `price×30/100`,
  truncated (0x4b9d34, 0x4b9f3e).
- **Selling** (0x4b9e5c): `Cost × ItemSaleCost × F / 10000`, truncated. Merchant adds half of that again, truncated.
  The relation does not change the sale price.
  - Only pack items with Cost > 1 can be sold (0x4abbfc), so personal items (negative price) cannot be sold.
- **Spells** are bought for exactly `CostGold`, with no relation factor and no Merchant (0x4ba3d4).
- **Barracks hire by the player** costs exactly the unit's `Cost` (0x4b0fcf), with no relation factor and no
  Merchant. Elementals are hired for their Cost in mana (0xc25dbd).
- **Ship**: `ShipCost` gold, read from the ini each time a ship is rented (0x4c60d0).

**Market stock** (0x4be178, run at every midnight for every building, 0x4a19e8, and at load): H/M.
- **Fixed goods** are the map's list, kept as negative ids in memory. They stay until bought.
- The **random goods** (positive ids) are thrown away and drawn again **every day at midnight**. The
  timer is set to now + 12 h, and the call comes at each midnight.
- Number of random goods = byte 295 minus the number of fixed goods still there.
- Price window: max = word 335 (capped at 5000, and +1 with chance 1/5), min = word 333 (at least 5, and not above max).
- **Towns (type 1)** get extra potions first:
  - `count div 5 + 1` healing potions (ids 98–100), one at random each.
  - If more than 6 random slots are left, one of them is instead id 96, 97, 114 or 115 (4 in 5 chance).
- The rest are drawn from items whose price is in the window, with a bias towards the window's
  upper part (the 0x4be4bf formula). A draw is refused if the same item is already stocked twice,
  or once when the max price is above 500. There are at most 25 tries.
- Finally the list is sorted by price, and ids above the item count are dropped.

**Barracks regrowth** (0x4a1998, at every midnight): H.
- Each of the 6 slots with `count < max` gains **+1 with chance 1/(MaxDayCountForNewUnit div max)**.
  When that divisor is 0 (max > 10), it always gains.
- With the default 10: max 5 → 50% a day, max 2 → 20% a day, max 1 → 10% a day.
- Random is an LCG: seed×0x343fd+0x269ec3, and `(seed>>16 & 0x7fff) mod n` (0x4832fc).

**Healing** (UI 0x494d63, action 0x4b12ef): H.
- Cost per unit = `max(1, Round((maxHP−HP)/maxHP × Cost × HealingConst/100 × 100/F))`. It is paid at once.
- Elementals pay in mana (Community).
- **No game time found for the player.** `HealingTime` is used only as an AI army's "busy until"
  (0x4a65b2, 0x4a6750): L/M.
- Where healing is allowed: the building's heal flag. Not decoded further.

**Resurrection**: H.
- Offered only when the hero is in a **town (1) or church (7)** and the unit is dead (0x494cfa).
- Cost = `Round(Cost × ResurectConst/100 × 100/F)` (0x494e2b, 0x4b1389). Elementals pay in mana.
- The unit comes back at full HP.
- A body is removed `MaxTimeResurection` minutes after death (0x4a55c8, time of death kept per unit).

**Daily heal effects**: H.
- Garrison units: +`GarrisonAutoHeal`% of max HP at midnight, wounded units only (0x4a1ce5).
- **Medic (bonus 4) anywhere in an army**: every wounded unit +**10%** max HP at midnight. This applies to all active armies (0x4a1dca).
- **Ranger hero**: every wounded unit +**15%** at noon (0x4ac277).

## 3. Villages, buildings, loot

**Tribute build-up** (0x4a1998, at midnight): H.
- If the maximum gold (word 284) is above 0: `stock += Round(income × √(1 − stock/max))`, capped at max.
- The mana stock follows the same rule, with bytes 350 (income), 351 (max) and a runtime stock byte.
- At load the stock is one day's income (0x4b55f0).
- Growth slows as the stock fills. With the linear rule the village fills in max/income days, and the
  real rule takes longer.

**Collecting**: H.
- On entering a village the hero takes all the gold and all the mana, with **no** F factor (0x4c6000).
- A hero whose first unit is Nature **Rogue** gets nothing from villages (0x4bbd69, 0x4bbde6).
- A village linked to a castle or fort (byte 293) that an army owns is **emptied into that owner's
  income at the owner's noon** (0x4a4302), and that includes the player.
- Castles and forts an army owns pay their stock at noon (×F/100 for the player). Their stock grows
  by the same √ rule if their max (284) is above 0.

**The 5 alternatives** (0x4bba40 chooses, 0x4aca80 builds the offer, 0x4ab966 applies): H.
- **Which villages offer them**: any village whose gold stock is above 0, when it is not the village
  where the last offer was made (that is cleared once tribute is taken). The map does not choose.
- Rolls are checked in this order, and the first that passes is offered. The option offered last time is never repeated.
  - **5 innkeeper**: roll 1/2; at least half the army unpaid; gold − wages + income < 0.
    Accepting marks everyone paid, with last paid = now.
  - **2 priest heals**: roll 1/3; total missing HP > 50; at least half the units wounded.
    It casts spell #1 (Исцеление, +30 HP to each unit) for free.
  - **1 long blessing**: roll 1/6; number of good spells in effect on the units ≤ army size.
    It casts one of spells **#3, 5, 7, 9, 11** (1-based: Гимн Жизни, Укрепление Брони, Святое
    Покровительство, Стремительность, Благословение), with the extended duration from §4.
  - **3 furs**: roll 1/6; pack holds < 25 items; at most 2 furs already.
    It gives item **135 "Пушнина"** (Cost 1000). With F = 100 it sells for 250, or 375 with a Merchant.
  - **4 witch**: roll 1/6; mana < gold; at least 3 spells known; no good spell in effect.
    It gives **+300 + 50·Random(5) mana** (300–500).
- Accepting any of them empties **both** the gold and the mana stock.

**Rumours**: L. No fixed rumour price was found in the code. Rumours are events, so the cost must be
the event's own gold result. The 10 gold seen in the footage would then come from the data.

**Loot after the player's win** (0x4c50ec): H.
- **Beating an army**:
  - The player gets `enemy gold div VictoryGoldDiv`. `MinVictoryGold` is **not used here**.
  - If the enemy's flag +0x3822 is 0 and its type is < 2, the player also gets **the enemy's daily wage total**.
  - The player takes every item the enemy units wore and everything in the enemy's pack.
  - If the enemy's home building is a castle or fort with an empty garrison, that building **becomes the player's**.
- **Beating a building's garrison** (castle, fort, ruins and others):
  - Gold = garrison gold + the building's stock + **one day's income** (word 282). No division.
  - The building's owner becomes the player, and its faction and attitude are copied from the player.
  - Ruins: the treasure gold (word 335) and items are given to the garrison at load (0x4b554e), so they come back through this rule.
- **Mana**: the loot mana is a value the battle code computes (0x66ae44 → 0x668ce8). It was not traced (battle agent).

**AI-vs-AI loot** (0x4a4e92): H.
- If the loser has less than `MinVictoryGold`, the winner takes all of it. Otherwise the winner takes `gold div VictoryGoldDiv`.
- So the minimum is a threshold, not a floor. A loser with 30 gold gives 15.
- The winner also gets the loser's wage total when the loser is type 0.

## 4. Spells

**Record** (0x49ae53): CostGold +0x26f, CostMana +0x273, school +0x277 (1 Life, 2 Elemental, 3 Death),
TimeWork +0x278, TimeCast +0x27c, Target +0x280 (1 Hero, 0 Enemy, −1 OneEnemy),
DeltaFixedHits +0x284, DeltaPercentHits +0x288, p-LifeLose +0x30c.

**Learning**: H.
- Costs exactly `CostGold`.
- The book holds **15** (the message is shown at 15, 0x4ba230). The array itself has room for 256.

**Casting from the book** (0x4c2e34, patch 0xc27448): H.
- Mana needed = `CostMana div d`; time = `TimeCast×2 div d` half-hour steps.
  - d = **2 for the Archmage hero**, 1 otherwise.
  - When d = 1 and a unit with **Caster** is in the army: floor(×0.8) instead.
  - **Archmage and Caster do not stack.**
- Time passes in 30-minute steps (0x4ae3c4, 3000 = 30 min).
- **Mana is taken when the spell completes** (0x4af564), not up front.
  - Enemy targets: the cast is abandoned if the target army goes inactive or hides (+0x16a1/+0x16a2) (0x4ae536).
  - Event-cast spells are free.

**Duration** (0x4900fc): H.
- The spell lasts exactly `TimeWork` hours. **No scaling by caster level** was found.
- Each unit has **4 spell slots**. Expired spells are cleared first.
  - **Casting a spell that is already on the unit adds another TimeWork** to what is left. The time is not reset.
  - With no free slot, nothing happens.
- **Event- or village-cast `Hero` spells on the player's army** end at now + TimeWork × **10**, or × **5** when TimeWork ≥ 8 (0x490557).
- Empty TimeWork = 0 = instant. There is no special case for 9999: it simply lasts 9999 h.

**Target handling**: H.
- `Enemy` and `Hero` hit every unit of the army.
- **`OneEnemy` hits only the first unit (the leader)**.
- A spell with **p-LifeLose ≠ 0 affects only the first unit**, whatever its Target.

**Instant effects**, applied on every cast:
- `DeltaFixedHits` is added to HP.
- `DeltaPercentHits` > 0 adds that % of max HP; < 0 takes that % of current HP.
- HP ≤ 0 → **the unit dies** (world spells can kill). HP ≥ max → unhurt.
- LifeLose < 0 cuts the unit's max-HP factor (unit+0x1bf) and its current HP by that %. LifeLose > 0 restores it.

**Enemy spell range**: L. The target is the army clicked, passing a check that was not decoded
(0x4ccb9c, local `[ebp-0xe]`). Armies with a "magic immune"-like flag (+0x3826 with +0x16af > 0) are refused.

**Potions**: H.
- A potion's modifiers last until the **end of the next battle**; they are cleared after each battle for the player's army (0x4c51a3 → 0x490720).
- `f-Hits` heals up to that amount.
- A potion with `f-Hits ≥ 1000` is the only thing that can be given to a **dead** unit (0x4976ce).

## 5. Items

**Stat order** (0x4908a8): H.
1. Base stats of the unit type at its level.
2. For each worn item in slot order, its `f-` value **replaces** the stat when above 0, so a later slot wins.
3. Potion `d-`.
4. Items' `d-`.
5. Spells' `d-`.
6. Potion `p-`.
7. Each item's `p-` **in turn, compounding**: `x = x + x·p/100`, truncated each time.
8. Each spell's `p-`, compounding the same way.
9. HP keeps its fraction of max HP (with a carried fraction). Below 1 → 1; above max → unhurt.

**Wear rules** (0x49765c): M.
- Potion and Item types cannot be worn.
- The dead can wear nothing (only the revive potion above works on them).
- A Shield or BlowWeapon needs a melee attack > 0.
- A ShotWeapon needs a ranged attack > 0, and a unit whose type's ranged value is above `ShotWeaponRange` (artillery) cannot use one.
- A Staff needs MagicPower > 0.
- Only one weapon, and no two items of the same type.
- **Undead cannot wear holy items**: ids 12, 46, 59, 72–77, 85, 94, 120, 131.
- **Item 154 (Королевская корона)** can be worn only by kind-0 units (the hero or a leader) or by unit types 1, 2, 3, 11, 13, 15, 36, 42, 45, 46, 48, 49, 53, 56, 58, 69, 70, 72, 73, 77, 89, 97 and 99.

**Pack**: H. 256 slots (0x49a724), shown as a scrolling grid 5 wide.

**Personal items**: M. A negative price means the item cannot be sold. How it is carried into the next map is in §6.

## 6. Event engine

Memory record: 171 bytes (0xab) per event, at `0x68ecec`. There are three extra slots after the last event:
N+1 is the noon report, N+2 the village offer, N+3 another dialog.

**Order of checks** (0x4abfbc): H.
1. **Global** events (type 1), in file order. The first one that passes is taken.
2. If none: the local events of the **event point** the hero stands on, in that point's list order.
3. If none: the local events of the **building** the hero is in, in the building's list order.
   - Only type-2 (local) events count here. **In villages and shipyards every listed event counts.**
   - A building's events are not checked again while that building's window is open (`0x4ed434`).
4. A silent event (no question and no message, 0x4a7a40) is applied at once, and the scan starts over.
   An event with a dialog stops the scan, and the scan resumes after the dialog.
5. When nothing more fires, events marked as fired "in the future" are set back to now (0x4ac369), and
   the noon processing runs (Ranger heal, report, wages).

**Time window** (0x4a7bcf): H. Fields: start +0x2 (minutes), repeat +0x6 (minutes, used as whole days),
duration +0x8 (hours), class +0xa, done +0x8c, once +0x8d, last fired +0x9c, times fired +0xa0.
- The event is never checked if done is set, or if start > now. This is how relative events wait.
- With repeat R: k = whole days since start. The event is open when `k mod (R/1440) = 0` and now lies
  within `[start + k days, + max(duration,1) hours]`.
- Without repeat: open while `now ≤ start + max(duration,1) h`.
- **Duration 0, no repeat, never fired** → open with no end.
- **Once flag** + fired at least once → closed.
- **Firing guard**:
  - Firing sets last fired = now + 1 and times fired += 1 (0xc28710, 0x4ab2a6). That blocks the same scan.
  - Duration > 0: the event may fire again **on any later check** while the window is open and the conditions hold.
  - Duration 0: it may fire again only 60 minutes after the last firing.
- Class: 0 means any; otherwise it must equal hero type + 1.

**Answers** (No: 0x4c23ce): H.
- **No** sets answer = 1, last fired = now + 1 and times fired += 1, and applies nothing.
  So a No **counts as happened**, and a once event is used up by a No.
- Opening the dialog resets answer = 0 (0x4a7ab8).
- Conditions (0x4a7d53…):
  - "happened with Yes": times fired > 0 and answer ≠ 1.
  - "happened with No": answer = 1.
  - "not happened": times fired = 0.
  - Each list holds 2 event ids, and all of them must match.

**Results** (0x4ab1ec): M.
- Flags are **counters**:
  - `+X` adds `X1`, or raises its digit if X is already set.
  - `-X` lowers the digit and removes X when it reaches 0 (0x4ab2d3).
  - A Community patch can set a chosen digit.
- A relative event is moved to **now + delay hours** (0x4ab89b).
- Spells go through §4 with the ×10 / ×5 duration.
- Added units are kind 3 (no wages).
- The gold and mana fields are added; mana does not go below 0.

**Campaign carry-over** (0x4b5b64, run when the victory event fires and `Maps_Rus\<next name>` exists): H/M.
The header bytes 0x110..0x116 decide:
- The hero's unit record always carries over.
- [0] **gold** is added.
- [1] **mana** is set to the old value.
- [2] fame: no code here.
- [3] **hero XP and level plus the whole spell book**. When it is off, the hero's XP and level are set to 0 and the book is not kept.
- [4] **the hero's 4 worn items**. When it is off, those slots are emptied. This means "worn items", not "negative-price items".
- [5] **the whole pack**.
- [6] **the whole army**: every unit is marked paid, with last paid = now.
- The next map is the header's next-map name. Community opcode 15 can change it.

---

## Razdor now → original

| Topic | Razdor now (mechanics.md §8, rules/*.rs) | Original (this exe) |
|---|---|---|
| Wage kind | Nature=Rogue → kind 2 | The player's units are always kind 1. Kind 2 only for AI hiring in foreign buildings. The hero and leaders are 0, event units are 3, and kinds 0 and 3 are free. |
| Short gold | Paid in order, the rest unpaid | Everything is deducted, then the **cheapest** units are refunded (unpaid) until gold ≥ 0, and gold is set to 0 |
| Desertion | After 7 unpaid noons in a row | When last paid + MaxTimeNotUpkeep < now, checked only on short noons |
| Garrison wage | Paid on the first noon | Never paid |
| Elemental wage | Paid in mana | Mana for kind 1 only. When mana runs out, the elementals go unpaid. |
| Rear Service | Not modelled / −30% | ×0.695 when the player's income ≠ 0, ×0.305 when it is 0 |
| Market markup | `max(0,1−att)×15%` | Table: 1.7 / 1.45 / 1.25 / 1.1 / 1.0 / 0.9 / 0.75 (own = 0.75) |
| Spell and barracks price | CostGold / Cost | The same, with no relation factor and no Merchant (Razdor matches) |
| Sale | ItemSaleCost% (+50% Merchant) | Also ×F/100 (F = 120 unless "impossible") |
| Market restock | Random part every 7 days | **Every midnight**. Towns also get healing potions. |
| Barracks regrowth | Deterministic, full in 10 days | Each slot +1 per day with chance 1/(10 div max) |
| Heal cost/time | ceil(...), HealingTime per unit | Round(...)÷(F/100), at least 1, **no game time** for the player (L/M) |
| Resurrection | Town or church, HealingTime | Town or church, ÷(F/100), instant |
| Medic / Ranger | 15% / 20% | **10%** at midnight (every army) / **15%** at noon |
| Village growth | +income per day | +income×√(1−stock/max) |
| Village options | All 5, every day | One per visit, chosen by the rolls and conditions in §3. Fixed spells #1 and #3/5/7/9/11, furs = item 135, witch 300–500 mana. A Rogue hero gets nothing. |
| Linked villages | Not modelled | Emptied into the linked castle owner's noon income |
| Victory gold | max(min, gold/div) | Player: gold div VictoryGoldDiv (no minimum) + the enemy's wage total. AI: all if < min, else div. |
| Castle capture gold | One day's income | Garrison gold + stock + one day's income |
| Empty home castle | Not modelled | Beating an army whose home castle or fort is empty takes that building |
| Spell cost modifiers | Archmage ×0.5 and Caster ×0.8 stack | Archmage ÷2, **or** Caster ×0.8 (only when not Archmage) |
| Mana payment | Before casting | When the spell completes |
| Recasting a spell | Restarts its time | **Adds** TimeWork to what is left; 4 slots per unit |
| OneEnemy | Like Enemy | Only the first unit (leader); p-LifeLose spells also hit only the leader |
| Instant spell damage | (capped) | Can kill |
| Village and event long spell | 3× TimeWork | ×10 (TimeWork < 8) or ×5 |
| Item p- | Summed | Compounded per item, then per spell |
| Item f- | Set first | Set first, later slot wins (Razdor matches) |
| Backpack | 40 | 256 |
| Spell book | 15 | 15 (Razdor matches) |
| "No" answer | Not "happened"; can be asked again | Counts as happened; a once event is used up |
| Repeat without once | Re-armed on a new window, failed check or new visit | Fires again on every later check while the window is open and the conditions hold |
| Duration 0 | No end | No end until the first firing, then a 60-minute refire guard |
| Flags | Set/clear | Counters (digit after the name) |
| Carry-over [3]/[4] | Level+XP / negative-price items | XP+level **and the spell book** / **worn items** |

## Unknowns left
- The enemy spell range check (0x4ccb9c).
- Surrender mana (computed by the battle code).
- Rumour price (probably the event's data).
- The player's heal time (probably none).
- The meaning of army type +0x16b7 in the wage and loot rules.
- Header byte [2] fame.
