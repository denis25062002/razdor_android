# The gameplay video against Razdor (a loose comparison)

An experiment, not a diff: the user's recording of Discord Times **1.5** playing the third map
of the first campaign (РК3, the capital), 40 minutes, compared with Razdor playing the same
coarse route on the user's install (**1.8 + Community Update 1.2**). Rules, data and the
generator's history differ between the two, so nothing here can match exactly; the point is
what is comparable across versions: the order of the windows, the events shown, the clock's
costs of the route, prices and rewards, the sides of the battles. Game texts are described
by meaning here, never quoted; events are named by their number in the map file.

## How it was done

**The video.** Frames sampled with ffmpeg (one every 5 s over the whole video as contact
sheets, the bottom bar cropped on each, 2-3 s grids around windows and battles; about 750
frames read by eye, the local qwen3-vl model was not needed). The transcription (date and
hour, gold, mana, income, wages, place, window, action, battles) is kept outside the repo
(the session's scratch folder), as it holds the video's texts. A few readings were checked
again on full frames (the bottom bar before and after the first two battles).

**The start.** The video begins at the map's own start minute (year 1204, month 5, day 19,
9 h, the same in 1.8), with what РК2 handed over: the archmage at level 5, 3154 gold, 1172
mana, a book of two spells (the heal and the blessing), about 25 items and an army of five.
РК3 cannot be started from New game (a later campaign map), so `razdor --replay` got a
direct start with a hand-given carry-over (README "Razdor side"; `new_game` with `carry`):

    {"op":"new_game","map":"РК3","hero":2,"carry":{"gold":3154,"mana":1172,"hero_level":4,
     "units":[[14,3],[28,3],[27,2]],"book":[1,11],"flags":["Band","King"],"reveal":true}}

It goes through Razdor's own campaign hand-over (`Game::from_campaign`), so the opening
events see it. Approximations: only the three paid units of the video's army whose type is
certain or matches the wage drop seen later (a cuirassier, a sorceress, a nun); the two
unpaid ones (event units) and the pack are left out; wages come to 296 against the video's
315. The two campaign flags were inferred from the rewards the video shows (below).
`reveal` marks the whole map explored, as for a player who knows it, so a click takes the
planner's route; with the fog, greedy walking took 34 hours to the capital instead of 27.

**The route** (`rk3-video.jsonl`, 38 actions, Razdor only): the village south-west of the
start, the capital (its opening chain), the town window, the blessing cast on the map, then
by land (the video sails: Razdor's replay has no ship purchase) to the bandit gang and the
castle north of the river, and waits to the next noon. Battles by `battle_auto`.

## What matched

| item | video (1.5) | Razdor (1.8 data) |
|---|---|---|
| start date and hour | 1204-5-19 9 h | the same |
| start to the first village | about 3 h | 3 h 37 min |
| village to the capital | 23 h (arrives 5-20 11 h) | 23 h 38 min (12 h 16 min) |
| opening chain at the capital | arrival, the king, three reports, the normal reward, the large reward with an item, the royal order, a secret task (two journal entries) | events 3, 5, 6, 7, 8, 11, 13, 14, 15, 26 in that order: the same windows; 15 and 26 are the two quests |
| rewards there | +1000, then +750 and a golden cup | +1000 (event 11), +750 and item 93 (event 13) |
| dialogs and town windows cost time | no | no |
| the noon report stops a walk that crosses noon | yes (on the way to the port) | yes (at 12 h 01 on the way in) |
| blessing cast on the map | -260 mana, 2 h | -260 mana, 2 h (520 / 4 h halved for the archmage) |
| boat price | 250 | `ShipCost` 250 (no purchase op) |
| tavern rumours in the main hall | -10 each | events 36-41: -10 each |
| hire prices seen | militia 50, bombardier 260 | 50, 260 (unit costs) |
| market prices | speed potion 175, healing potion 35 | 250 and 50 less 30% (Merchant bonus): 175, 35 |
| bandit gang's army | 2 units (one marauder) | 2 units (a marauder and a monk) |
| castle taken from its garrison | +250 gold, income 0 → +125 | +250, income 0 → +125 |
| first noon after taking a castle | its income not paid (-240 = wages) | not paid (-296 = wages): the castle's gold stock starts at 0 |
| later noons | income paid 125 even with forts owned (shown +155/+175) | economy.md §1/§3: only castle and fort *stocks* are paid, forts with max 0 never fill; Razdor pays the castle's 125 (5-23: -171 = 125 - 296) |

The large reward is the strongest check: it needs events 7 and 8, whose title scripts require
the campaign flags `Band` and `King` set on the earlier maps. Without the flags Razdor gives
only the +1000; with them it gives the video's +1000, +750 and the cup. So the video's
player had both flags, and Razdor's flag hand-over and event chain agree with 1.5 here.

## What differed

| item | video | Razdor | likely cause |
|---|---|---|---|
| sanctuary prices at the capital | 300, 225, 430, 240, 200, 220 | 150, 125, 220, 240, 100, 110 | **version (data)**: the same six spells in the same order; the 1.8 install's `CostGold` values, about half for five of them |
| first village | an offer (furs instead of the tribute) | tribute paid (+90 gold, +80 mana) | **generator**: the offer is rolled at entry (economy.md §3); the histories differ |
| village window | a single message with the tribute icons | the village window | **version (interface)**, or the transcription's reading; not checkable without 1.8 footage |
| capital market goods | had the two potions bought | other goods (random restock) | **generator** |
| time to the capital | 26 h | 27 h 15 min (+38 min on the first leg, +15 min for the noon stop) | **route or version**: the video's first leg is shorter (a slightly different start cell or road in 1.5); the second leg agrees within 40 min |
| gold for the bandit gang | +127 | +174 (125 of the army's gold + 49) | **version**: settled by the diff test (FINDINGS §19, `lake-gang.jsonl`): 1.8 + Community pays the beaten army's wage bill (+0x16e0, the bill of its last recount) on top of its gold share, a gang of Проклятое озеро +75 + 85 on both sides. Razdor's +174 is the 1.8 rule (the gang's bill of 49, its leader drawing none); the video's 1.5 gives +127, about the gold share alone (no bill, or a gold of 254). Razdor had one slip here, the bill counted after the battle (a gang wiped out paid none): fixed |
| mana from the battles | gang 0, castle +20 | gang +20, castle 0 | **the battles' course**: the mana comes from enemies that surrender (`Surrender`); Razdor's auto battle is not the player's battle |
| route after the capital | by boat upriver | on foot, round the west (the replay cannot buy a ship) | **harness**: no shipyard op in the action list |

## Weak points

- The carry-over is a guess for everything the video does not show: unit levels, the two
  unpaid units, the pack, the hero's worn items, XP; Razdor's wages are 19 lower.
- The transcription is a reading of small, compressed frames; some values are marked as
  inferred there (several enemy units, the gold of one battle).
- Only the first video day is replayed closely; after the capital the routes part (boat).
- One generator, two histories: anything rolled (offers, markets, the AI's moves, battles)
  is not comparable, only its kind.
