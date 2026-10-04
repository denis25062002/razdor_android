# Playtest notes

Things noticed while playing Razdor, to look into. Newest first. Each note says which branch it
was seen on and what to check.

## 2026-10-04, dt-original: a quest's places shown only after leaving the building

1. **"When I take a mission in the barracks, the map with the quest's places pops up not
   during the dialog but only after I close the barracks window; it should show when
   needed."** Done. Checked under Wine on РК1 (`tools/difftest/rk1-castle-quest.jsonl`: the
   castle's main hall, «Сообщение посыльного», lantern 2): **the original flies at the
   quest's OK**: its OK queues the glide, the reveal and the glide back (Event_Finish 0x4ab1ec
   → 0x4af96c, 0x4af83c), the screen switches to the world map while they play (camera
   y 440 → 264 → 440 in about 2 s), then the building window comes back on its tab, silent;
   closing it later moves nothing (Frida hooks on 0x4af96c/0x4af83c fire only in the OK's
   step; screenshot burst). Razdor waited for the building window to close, because the map
   frame that plays the flights did not run under it. Now the building window steps aside
   for the flights (input off meanwhile) and comes back as it was (`App::fly_from_building`;
   interface.md §9.8, events.md §10). The diff test got a `take` op for the main hall on both
   sides. Commit b41b916.

## 2026-10-03, dt-original: a hero class the map leaves out

1. **A player's report: "on 'Осмотр владений' I started as the Ranger though only the Knight
   was meant to be playable."** The original does not allow this in a new game: a class is
   offered only when its preset has a start cell, its portrait is disabled otherwise and takes
   no click or key, and the window opens on the first offered class (interface.md §5,
   saves-data.md §10.4; checked under Wine on Устье Трейна, whose archmage is left out).
   Razdor's hero window let every class be picked on every map; dt-original now offers only
   what the original offers. The original's real gap is a campaign: the next map keeps the
   class without checking that the map offers it, and the hero then starts at cell (0,0) of the
   empty preset. **When dt-feat merges dt-original, treat that as an original bug to fix:
   offer only the classes the map defines**, and on a campaign's next map that leaves the
   class out, do not drop the hero at (0,0) (for example, refuse the map in the editor's
   checks or start him on the first offered class's cell).

## 2026-10-03, dt-feat with the Community Update install

1. **Windows open on top of each other.** Several windows fire at once and stack, one over the
   other. To check: which windows (event messages, building windows, battle, reports at noon),
   in what order the original shows them, and whether it queues them one at a time. See
   interface.md (message boxes, the order of the world-map windows) and events.md (the ask/OK
   flow).
   Checked (2026-10-04, dt-original): the original shows one window at a time by
   construction: the event scan opens one event's window and runs again only when it is
   finished (or answered No); the noon report is part of that scan; a building reached as a
   window opens waits for it (0x4ed42c); a battle and its report come after the windows of
   that moment. Razdor queues its dialogs the same way (one shown, the next after it). To find
   what still differs, every action list of the diff-test runs so far (28 lists, about 550
   steps) was replayed in Razdor with its screen read at each step and set against the
   original's screen (event window, village, building, battle, map). Two differences, both
   fixed: (1) an event's window that cut the walk short inside the clicked building: the
   original opens the building's window after the OK (РК1, runs r3-c004157 and
   rk1-h2-minimap; 0x4aed41 → 0x4ae5d8, 0x4aed64), Razdor left the hero on the map;
   (2) after a heal, a raise, a purchase or a sale in the building window the original checks
   the events as the window closes (0x4ed440, 0x4b8f63), Razdor only at the next step. The
   other screen differences of those runs come from AI walks that part (FINDINGS §5) or from
   `battle_auto`. No window of dt-original was found open over another one; what was seen on
   dt-feat should be checked again after it merges. Commit 50dcb52.

2. **Entering a village must not make it the hero's.** The player only takes the village's
   money, and only if nobody else has taken it that day. This **contradicts the current spec**:
   world.md §6 and economy.md ("Entering a village", 0x4bbc84) say the original captures an
   unguarded village when the hero steps on it, and Razdor follows that. To check: re-read
   0x4bbc84 and the capture in world.md against the original under Wine (enter a village, look
   at its owner and its tribute; then let an AI army take the tribute first and enter on the
   same day). Fix whichever side is wrong, on both branches.
   Checked (2026-10-04, dt-original): **the original does capture the village**, and Razdor is
   left as the original. Read live from the original's building records (owner +0x124, stock
   +0x11e) in the diff test: the hero's step into a neutral village makes it the player's (ДС1
   village 13: 255 → 0; Проклятое озеро villages 2 and 30: 255 → 0), and an AI army's capture
   is undone the same way (`tools/difftest/rk1-village-taken.jsonl`, run n2-village-taken: on
   РК1 army 9 takes the hero's start village 6 at 13:00, owner 9, gold 60 → 0, mana 15 → 0;
   the hero walks in at 18:30 the same day: owner 0, the village window pays nothing, his gold
   stays 100). The "only if nobody else took it that day" part already holds: the tribute is
   the village's stock, which the army that came first emptied and which refills at midnight
   (economy.md §3). Razdor does the same (test
   `rk1_a_village_emptied_by_an_army_pays_the_hero_nothing_that_day`; in the free run the two
   games' AI walks part before the village, FINDINGS §5, so army 9 meets Razdor's hero on the
   way). If the wish stands (villages never change hands for the player), it is a change of
   the original's rules, for dt-feat. Commit a63970b.

3. **Missing animations: units in battle and levelling up.** Fights lack the units' animations,
   and a level-up has none. To check: which battle animations the original plays (attack,
   shot, spell, hit, death) and the level-up effect, from the install's art (Graphics/Battle,
   Graphics/Spells) and interface.md / engine.md (animation timings). Presentation was left out
   of the parity pass on purpose, so this is open work, not a regression.
   Measured (tools/difftest/AV.md): the original has no animated unit figures; its battle
   effects match Razdor's one for one except the counterblow's slide back and effect, and the
   level-up shows only in the won battle's 2.5 s hold (experience cards) and the promotion
   screen; the hold is missing in Razdor.
   Done (2026-10-04, dt-original): the counterblow's lunge back with its effect and sound on
   the attacker (and the sorcery on a killer a DeathCurse unit takes along), the won battle's
   2.5 s hold with the experience on the cards and no result box, a pass's 100 ms pause, and
   no level-up sound outside the promotion screen. The AV runs now match the original's battle
   sounds and effects step for step (AV.md); unit sprites are not part of the original.

4. **No ranged defence (Защита стрелковая) on the back row.** Seen in battle: units in the back
   row show or get no ranged defence. The spec says the original adds Row2Def (+5 in the
   shipped `_Global.ini`) to a row-2 target's defence against shots, after any piercing, in the
   damage formula (battle.md, Row 2 defence, 0x485a04). To check: whether Razdor applies the +5
   in the damage (a test on a row-2 target hit by a shot), and whether the original shows it on
   the unit card and panel while Razdor does not (display only). Compare the card of the same
   back-row unit in both games.
   First finding: Razdor does apply it in the damage (`src/rules/battle.rs`, `row2_def` added for
   a row-2 target of a shot), so this is most likely the card and panel not showing the bonus.
   Done (2026-10-04, dt-original): the original adds Row2Def to what it shows of a unit in a
   back-row place (7-10) on its card strip ("D: m/r", 0x49462c) and on its panel ("v + n", n =
   building defence + Row2Def, 0x492f24 sets the row flag, 0x491fa4 writes it), in battle and
   on the army and building screens (not on a recruit offer). Seen under Xvfb in РК1's ruins
   battle: the novice and the archer of the back row show "D: 0/5", the panel "5 + 3" for a
   guard in its building. Razdor now shows the same on the card strips (battle, army, building
   windows) and in the panel's ranged defence line; the damage was already right. Commit f5ed454.

5. **Feature request: a setting for the front row's width.** In the settings, a choice between a
   wide front row (6 cells) and a short one (4 cells). With the short row, the 2 edge cells of the
   front row become inactive cells, as the back row's edge cells already are. Today the width
   comes from the install's `OptValue11` (wide by default, see the restored "wide row" choice and
   battle.md §6), with no in-game switch. To work out: where the setting lives (Razdor's
   `settings.json` vs the install's option), whether it applies to a battle or a whole game
   (saves record the row width), and how the reserve row changes with it.
   Done (2026-10-04, dt-original): the short row is the original's own 4-column formation
   (`OptValue11` = 0): front 4, back 4, reserve 4, and the original draws it on the same 2 × 6
   places (0x492940): front and back rows in the middle four, the reserve's four cells at the
   ends of both lines, so the front row's edge places are inactive reserve cells exactly as
   asked. The width is a whole game's (stored in the save at a new game, 0x4b25a2). Razdor's
   settings window now has "Front row in battle (new games)": 6 or 4 cells, kept in
   `audio.json` with the other settings (`wide_row`; until chosen the install's `OptValue11`),
   applied to games started afterwards; saves keep their width. The 4-column formation is now
   drawn as the original's places (it was three lines of four). Commit 7019060.

6. **The camera jumps back to the hero on the first click.** With the hero off screen (the map
   scrolled away), a single click on a place moves the view straight back to the hero. Wanted:
   the view stays where it is while the route is chosen, so a second click on the same place
   (the route preview's confirm, a double click) can be made there; only once the hero starts
   walking does the view go back to him. To check: what the original does (whether its camera
   follows the hero only while he walks), and in Razdor the camera-follow logic in
   `src/ui/world_view.rs` (the camera following the hero unless moved by the minimap) and the
   route preview's first click.
   Done (2026-10-04, dt-original): the original does what is wanted. Its click handler writes
   the camera only for a minimap drag and the arrow keys (0x4ccf5a-0x4cd00f); only the walk
   locks the view on the hero (0x4ae8a8). Checked under Wine on РК1: with the view scrolled
   400 px off the hero, the first click drew the route and left the camera at (210, 440); the
   second set him off and the camera went to (608, 440). Razdor reset the view on every
   click on a target; now a click leaves it and the walk brings it back (`camera_look`), so
   this is parity, not a Razdor choice. Commit 60e1eaf.

7. **Second campaign map: the "send the peasants to the mines" offers.** There are three offers to
   send a group of peasants to the mines. The player accepted two and declined one. The declined
   offer never came back, although all three should be accepted (the declined one asked again).
   After the second accepted group, the quest was reported as completed, although only two of
   three groups were sent. To check, on both branches: the events behind these offers on the
   second map (their repeat and once flags, the "No" result, the follow-up and the quest's
   completion condition), against events.md (the ask / Yes / No flow, which results apply on No,
   repeats and the once flag) and the original under Wine. Related: dt-feat fixed the original's
   bug "a repeating question without a message asks again next time" (events step), so the
   branches may differ here; and whether the quest's completion counts groups sent or fires on
   another condition.
   **To fix** (the user, 2026-10-03): the quest must follow the original's offers and completion.
   Checked (2026-10-04, dt-original): Razdor already follows the original here; no code change.
   The map's events (village building 4): offer 8 asks with the baron's promise (5) answered
   Yes; offer 9 needs Yes to 5 and 8 and, on its Yes, opens offer 10 a day later; 10 asks only
   while the army has no peasant left (three "not the player's" unit slots) and the quest's end
   (27) has not fired: it is a replacement, not a third group. 8, 9 and 10 are many-times
   events with a message. In the original a No only counts the firing (answer 1, times + 1,
   last fired = now + 1: 0x4c2320), so a declined offer is not asked again in that visit and is
   asked again when the hero next enters the village; a Yes makes it a once-event (0x4c2100).
   Each mine's fort takes three peasants (19, 24; "three per mine" in the baron's own words),
   each completing its mine's quest (18, 23), and 27 completes the campaign quest (4) only
   after both: two groups of three are the whole task, so the quest done after the second
   accepted group is the original's. The original cannot start РК2 outside the campaign (New
   game lists only first maps), so this was checked against its code (events.md §2, §6.2) and
   the map file, and played in Razdor with the replay's carry-over (the herald now carried by
   `named`): test `rk2_the_peasant_offers_and_the_mines` declines 8 and 9 and gets them back on
   the next visit, staffs the north mine (quest 18 done, 4 not), loses the other three, gets
   offer 10 a day later and staffs the south mine (27 fires, quest 4 done). What dt-feat
   changed for repeating questions should be checked against this test when it merges.
   Commit 0d4057a.
