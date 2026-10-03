# Sounds and visual effects: coverage (original vs Razdor)

Measured with `run.py --av` (README.md, "Sounds and animations"): the original's sounds,
tracks and timed animations from the Frida preset `av`, Razdor's from its replay log
(`src/av.rs`), compared step by step on the step-local run. Runs of 2026-10-04 (folders
under `~/.cache/razdor-difftest/runs/`):

| run | list | steps | steps alike | sfx missing / extra | anim missing / extra |
|---|---|---|---|---|---|
| `av-rk1` | `rk1-day1.jsonl` (РК1, knight: village, events, waits, the ruins' battle, noon) | 44 | 30 | 10 / 5 | 7 / 0 |
| `av-rk1-heal` | rk1-day1 then the church at (47,27): heal ×2, sell, learn | 49 | 29 | 22 / 9 | 12 / 3 |
| `av-ds1-services` | `ds1-services.jsonl` (ДС1, knight: church: buy ×3, hire ×2, learn, sell, equip) | 15 | 1 | 26 / 14 | 13 / 9 |
| `av-lake-cast` | `lake-cast.jsonl` (Проклятое озеро, archmage: re-entered building, village offer, hire ×2, two casts) | 25 | 6 | 27 / 19 | 7 / 0 |

Music: no difference in any run (map theme, battle theme, triumph, the track after the
victory box).

How to read the table: *original* and *Razdor* count the plays over the four runs; *both* =
seen on both sides (the counts can still differ, see the gaps below), *missing in Razdor* =
the original played it and Razdor never did, *only Razdor*, *never triggered yet* = no run
reached it. Names: the `_Sounds.ini` keys (`[SFX-Effects]`, `[Backgrounds]`), and for the
animations the callbacks the game pushes on its deferred-call queue (`av.QUEUE_FNS`).
32 sounds, 13 tracks, 18 animation kinds: sounds 14 both, 3 missing in Razdor, 1 only
Razdor, 14 never triggered; tracks 5 both, 8 never triggered; animations 8 both,
6 missing in Razdor, 4 never triggered.

| kind | name | original | Razdor | status | in the original |
|---|---|---|---|---|---|
| sfx | `InterfaceButtonDown` | 50 | 46 | both | press of almost every standard button (dialog OK/Yes/No, wait buttons, Next/Start, list switch) |
| sfx | `InterfacePanelDown` | 3 | 14 | both | press of a bottom panel icon (army, book...) |
| sfx | `InterfaceCastSpell` | 33 | 2 | both | a world spell cast; **a building window tab highlighted** (also when the window opens on its first tab); load window tabs |
| sfx | `InterfaceBarScroll` | 0 | 0 | never triggered yet | options slider test |
| sfx | `MainMenuSelect-1` | 4 | 0 | missing in Razdor | hover of a main menu item (-2, -3 share its slot) |
| sfx | `MainMenuPress` | 5 | 12 | both | press of a main menu item; class portrait |
| sfx | `Global-Event-1` | 9 | 8 | both | event, village or shipyard window opens (Random(3)) |
| sfx | `Global-Event-2` | 8 | 9 | both | as above |
| sfx | `Global-Event-3` | 13 | 13 | both | as above |
| sfx | `Global-Battle` | 2 | 2 | both | battle window opens |
| sfx | `Unit-Upgrade` | 0 | 0 | never triggered yet | the promotion screen only (0x4b1af8) |
| sfx | `Spell-Good` | 2 | 2 | both | world spell lands on the own army |
| sfx | `Spell-Evil` | 0 | 0 | never triggered yet | world spell lands on another army |
| sfx | `Battle-Fight` | 30 | 28 | both | melee / long strike effect |
| sfx | `Battle-Shoot` | 12 | 12 | both | shot effect |
| sfx | `Battle-Cure` | 2 | 0 | missing in Razdor | heal effect; heal or raise in a building (0x4b129c) |
| sfx | `Battle-Bless` | 6 | 6 | both | bless effect (also a heal on an unhurt unit) |
| sfx | `Battle-Strike` | 0 | 0 | never triggered yet | cannon shot (range ≥ ShotWeaponRange) |
| sfx | `Battle-Sorcery` | 0 | 0 | never triggered yet | curse / magic strike; DeathCurse or Ghost on the killer |
| sfx | `Card-Move` | 4 | 0 | missing in Razdor | card slide: battle move, army exchange, building grids, hiring |
| sfx | `Item-Item` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-BlowWeapon` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-ShotWeapon` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Armor` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Helm` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Shield` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Staff` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Amulet` | 2 | 3 | both | item of that type picked up, dropped or worn |
| sfx | `Item-Ring` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Potion` | 0 | 1 | only Razdor | item of that type picked up, dropped or worn |
| sfx | `Item-Gold` | 21 | 10 | both | Trade button, hire buttons, event dialog button, village tribute taken, ship bought |
| sfx | `Battle-Parry` | 0 | 0 | never triggered yet | Community; no ini entry, never played by the shipped install |
| music | `BkgMenuMain` | 0 | 0 | never triggered yet | main menu |
| music | `BkgAuthors` | 0 | 0 | never triggered yet | credits; rotation pick 3 |
| music | `BkgMap1` | 1 | 1 | both | map rotation pick |
| music | `BkgMap2` | 4 | 4 | both | map start / load (then the rotation) |
| music | `BkgMap3` | 0 | 0 | never triggered yet | map rotation pick |
| music | `BkgMap4` | 0 | 0 | never triggered yet | map rotation pick |
| music | `BkgMap5` | 0 | 0 | never triggered yet | map rotation pick |
| music | `BkgMap6` | 1 | 1 | both | map rotation pick |
| music | `BkgMap7` | 0 | 0 | never triggered yet | map rotation pick |
| music | `BkgBattle1` | 2 | 2 | both | battle against a garrison |
| music | `BkgBattle2` | 0 | 0 | never triggered yet | battle against an army |
| music | `BkgTriumph` | 2 | 2 | both | battle won |
| music | `BkgDefeat` | 0 | 0 | never triggered yet | battle lost / army wiped by a spell |
| anim | `army_slot_slide` | 4 | 0 | missing in Razdor | 0x4b0c04: a unit moved between armies or hired |
| anim | `battle_effect:bless` | 6 | 6 | both | 0x4afe7c picture 3 |
| anim | `battle_effect:cure` | 0 | 0 | never triggered yet | 0x4afe7c picture 4 |
| anim | `battle_effect:magic` | 0 | 0 | never triggered yet | 0x4afe7c picture 2 |
| anim | `battle_effect:melee` | 30 | 28 | both | 0x4afe7c picture 1 (also cannon) |
| anim | `battle_effect:shot` | 12 | 12 | both | 0x4afe7c picture 0 |
| anim | `battle_end_hold` | 2 | 0 | missing in Razdor | 0x4b09e8: the won battle's 2.5 s hold with the experience cards |
| anim | `battle_pass` | 6 | 0 | missing in Razdor | 0x4afb54: a pass, 100 ms busy pointer |
| anim | `battle_slide` | 48 | 46 | both | 0x4afbd8: action sprite actor → target (counter: back) |
| anim | `camera_glide` | 12 | 11 | both | 0x4af96c: 900 ms glide (shown places, centre button) |
| anim | `card_slide` | 0 | 0 | never triggered yet | 0x4b0284: a battle move |
| anim | `look_at_army` | 2 | 0 | missing in Razdor | 0x4afa98: camera to a spell's target army |
| anim | `promotion` | 0 | 0 | never triggered yet | 0x4b1a04: the promotion screen |
| anim | `reveal` | 11 | 7 | both | 0x4af83c: the fog opening (map start, shown places) |
| anim | `unit_action` | 2 | 0 | missing in Razdor | 0x4b11cc: heal, potion or dismiss in a building |
| anim | `wait` | 12 | 12 | both | 0x4ae280: a wait, an event's delay, a spell's reading |
| anim | `walk` | 22 | 22 | both | 0x4ae6dc: the hero's walk |
| anim | `world_spell` | 2 | 0 | missing in Razdor | 0x4af2f8: a world spell's effect on an army |

## The gaps, by weight

1. **Battle: the counterblow is silent and unseen in Razdor.** The original plays the
   attacker's slide back and a second effect with its sound on the attacker (РК1 step 30:
   `battle_slide`, `battle_effect:melee` on 1:1:3 and a second `Battle-Fight`); Razdor shows
   only the counter's number. Every other battle action matched by kind, card and order
   (all the other slides, effects and their sounds of the ruins' battle: the blesses, the shots,
   the AI's blows).
2. **Battle: the won battle's hold and the experience cards** (`battle_end_hold`, 2.5 s,
   interface.md §9.9) are missing; Razdor shows its result box at once and needs an OK press
   (`InterfaceButtonDown` extra at the battle's last step). Passes (`battle_pass`, 100 ms busy
   pointer) have no counterpart.
3. **Building windows.** The original highlights the window's tab with `InterfaceCastSpell`
   when the window opens and on every tab switch; Razdor plays `InterfacePanelDown` on the
   window's opening and nothing on a tab switch (each run; the number of tab sounds also
   depends on how many tab presses the harness makes).
4. **Trade, hire, heal, learn.** The original's service buttons play `Item-Gold` (Trade,
   hire ×2 per unit: two call sites, learn) instead of the button sound; hiring adds the
   unit's slide into the army (`army_slot_slide`) with `Card-Move`; healing in a building
   plays `Battle-Cure` with `unit_action`. Razdor plays `InterfaceButtonDown` for all of
   them, an `Item-<type>` on a purchase (the original plays none there: `Item-Potion`
   is Razdor's only), and `Item-Gold` only when gold rises (a sale, a tribute, midnight
   income: extra at rk1-day1 step 42).
5. **Village tribute.** The original plays `Item-Gold` when the tribute is taken (the
   window's button or its close, 0x4c604a, one step later); Razdor plays it as the window
   opens (rk1-day1 steps 3/4, lake steps 4/5, 10/11).
6. **World spells.** `look_at_army` (the camera to the target) and `world_spell` (the effect
   on the army) are missing; the sounds (`InterfaceCastSpell`, `Spell-Good`) match.
   Razdor adds the bar button's `InterfaceButtonDown` where the original's panel icon
   plays only `InterfacePanelDown`.
7. **Shown places.** The original flies to an event's places (and back to the hero) right
   after that event's OK; Razdor waits until every queued window is closed and then flies
   to all of them (ДС1 steps 1, 3 vs 4; РК1 heal steps 42 vs 44). The map start's
   `reveal` around the hero (radius 18) is missing.
8. **Waits.** The original's wait buttons play `InterfaceButtonDown`; Razdor's time panel is
   silent (every wait step).
9. **Main menu.** `MainMenuSelect-1` on hover is missing; Razdor's scenario Next and Start
   play `MainMenuPress` where the original plays the button sound (extra `MainMenuPress` ×2
   at every new game).
10. **Equip.** The original plays the item's sound twice (taken from the pack, worn);
    Razdor once.

Not triggered yet (need runs that reach them): `Unit-Upgrade` (the original plays it only
in the promotion screen, 0x4b1af8; Razdor plays it on every level gained, after a battle
and on a level from an event: expected *extra in Razdor* once a run gains a level),
`Spell-Evil`, `Battle-Strike`, `Battle-Sorcery`, `Battle-Cure` in battle, the item types
other than amulet and potion, `BkgBattle2`, `BkgDefeat`, `BkgMenuMain`/`BkgAuthors`, the
other map tracks, `card_slide` (a battle move), `battle_effect:magic`/`cure`, `promotion`,
the DeathCurse/Ghost sorcery effect on the killer, `Battle-Parry` (never: no ini entry).

## Not seen by the trace

The trace sees what starts; continuous animations are rates in the frame code
(engine.md §7, interface.md §15) and are compared there: the hero's walk frames (original
frames 3–6 at half the WalkDelay, Razdor 8 frames at 10 per second), the AI armies' walk by
game time and their 20-step idle, water (32 frames at 100 ms, Razdor static), tree sway,
selection ring, route arrows, card pulse, cursors (Razdor: system cursor). Cross-fades of
the music (2 s, 4 s) are left out in Razdor by choice. There is no death animation in the
original: a dead unit's card goes when the effect that killed it ends.

## PLAYTEST_NOTES.md note 3 (missing unit animations in fights and on level-up)

Measured: the original has no animated unit figures in battle at all. A battle action is
the slide of a sprite from the actor's card to the target's and a 25-frame effect over
the target (melee, shot, magic, bless, cure; 350 ms), with its sound; Razdor has both (the
actor's card lunges in place of the slide, its own effect art) and they match one for one,
except the counterblow's slide back and effect on the attacker (gap 1) and the DeathCurse
or Ghost effect on the killer (not reached yet). The level-up has no animation in the
original either: during the 2.5 s hold after a win each card that gained shows the
animated "Expirience +N" strip and the promotion marker (gap 2), and `Unit-Upgrade` plays
only in the promotion screen. So the work for note 3 is gaps 1 and 2, not unit sprites.
