# Sounds and visual effects: coverage (original vs Razdor)

Measured with `run.py --av` (README.md, "Sounds and animations"): the original's sounds,
tracks and timed animations from the Frida preset `av`, Razdor's from its replay log
(`src/av.rs`), compared step by step on the step-local run. The original's side was recorded
on 2026-10-04 (folders `av-*` under `~/.cache/razdor-difftest/runs/`); Razdor's side is
re-run on it with `--reuse-original <run>/original` (folders `av-*-final`, after the gaps
below were closed):

| run | list | steps | steps alike | sfx missing / extra | anim missing / extra |
|---|---|---|---|---|---|
| `av-rk1` | `rk1-day1.jsonl` (РК1, knight: village, events, waits, the ruins' battle, noon) | 44 | 44 | 0 / 0 | 0 / 0 |
| `av-rk1-heal` | rk1-day1 then the church at (47,27): heal ×2, sell, learn | 49 | 48 | 1 / 1 | 0 / 0 |
| `av-ds1-services` | `ds1-services.jsonl` (ДС1, knight: church: buy ×3, hire ×2, learn, sell, equip) | 15 | 15 | 0 / 0 | 0 / 0 |
| `av-lake-cast` | `lake-cast.jsonl` (Проклятое озеро, archmage: re-entered building, village offer, hire ×2, two casts) | 25 | 23 | 2 / 2 | 0 / 0 |

Before the fixes the same runs had 30, 29, 1 and 6 steps alike. The three steps left differ
only in the chord a window drew (`Global-Event-k`, the game generator's `Random(3)`: rk1-heal
step 41, lake steps 10 and 16): Razdor's generator is elsewhere at that moment, a rules
matter (the draws before the window), not a sound missing.

Music: no difference in any run (map theme, battle theme, triumph, the track after the
victory box).

How to read the table: *original* and *Razdor* count the plays over the four runs; *both* =
seen on both sides (the counts can still differ), *missing in Razdor* = the original played
it and Razdor never did, *only Razdor*, *never triggered yet* = no run reached it. Names:
the `_Sounds.ini` keys (`[SFX-Effects]`, `[Backgrounds]`), and for the animations the
callbacks the game pushes on its deferred-call queue (`av.QUEUE_FNS`).
32 sounds, 13 tracks, 18 animation kinds: sounds 17 both, 15 never triggered; tracks 5
both, 8 never triggered; animations 14 both, 4 never triggered. Nothing is missing in
Razdor or only Razdor's any more.

| kind | name | original | Razdor | status | in the original |
|---|---|---|---|---|---|
| sfx | `InterfaceButtonDown` | 50 | 50 | both | press of almost every standard button (dialog OK/Yes/No, wait buttons, Next/Start, list switch) |
| sfx | `InterfacePanelDown` | 3 | 3 | both | press of a bottom panel icon (army, book...) |
| sfx | `InterfaceCastSpell` | 33 | 33 | both | a world spell cast; **a building window tab highlighted** (also when the window opens on its first tab); load window tabs |
| sfx | `InterfaceBarScroll` | 0 | 0 | never triggered yet | options slider test |
| sfx | `MainMenuSelect-1` | 4 | 4 | both | hover of a main menu item (-2, -3 share its slot) |
| sfx | `MainMenuPress` | 5 | 5 | both | press of a main menu item; class portrait |
| sfx | `Global-Event-1` | 9 | 8 | both | event, village or shipyard window opens (Random(3)) |
| sfx | `Global-Event-2` | 8 | 9 | both | as above |
| sfx | `Global-Event-3` | 13 | 13 | both | as above |
| sfx | `Global-Battle` | 2 | 2 | both | battle window opens |
| sfx | `Unit-Upgrade` | 0 | 0 | never triggered yet | the promotion screen only (0x4b1af8) |
| sfx | `Spell-Good` | 2 | 2 | both | world spell lands on the own army |
| sfx | `Spell-Evil` | 0 | 0 | never triggered yet | world spell lands on another army |
| sfx | `Battle-Fight` | 30 | 30 | both | melee / long strike effect |
| sfx | `Battle-Shoot` | 12 | 12 | both | shot effect |
| sfx | `Battle-Cure` | 2 | 2 | both | heal effect; heal or raise in a building (0x4b129c) |
| sfx | `Battle-Bless` | 6 | 6 | both | bless effect (also a heal on an unhurt unit) |
| sfx | `Battle-Strike` | 0 | 0 | never triggered yet | cannon shot (range ≥ ShotWeaponRange) |
| sfx | `Battle-Sorcery` | 0 | 0 | never triggered yet | curse / magic strike; DeathCurse or Ghost on the killer |
| sfx | `Card-Move` | 4 | 4 | both | card slide: battle move, army exchange, building grids, hiring |
| sfx | `Item-Item` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-BlowWeapon` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-ShotWeapon` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Armor` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Helm` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Shield` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Staff` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Amulet` | 2 | 2 | both | item of that type picked up, dropped or worn |
| sfx | `Item-Ring` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Potion` | 0 | 0 | never triggered yet | item of that type picked up, dropped or worn |
| sfx | `Item-Gold` | 21 | 21 | both | Trade button, hire buttons, event dialog button, village tribute taken, ship bought |
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
| anim | `army_slot_slide` | 4 | 4 | both | 0x4b0c04: a unit moved between armies or hired |
| anim | `battle_effect:bless` | 6 | 6 | both | 0x4afe7c picture 3 |
| anim | `battle_effect:cure` | 0 | 0 | never triggered yet | 0x4afe7c picture 4 |
| anim | `battle_effect:magic` | 0 | 0 | never triggered yet | 0x4afe7c picture 2 |
| anim | `battle_effect:melee` | 30 | 30 | both | 0x4afe7c picture 1 (also cannon) |
| anim | `battle_effect:shot` | 12 | 12 | both | 0x4afe7c picture 0 |
| anim | `battle_end_hold` | 2 | 2 | both | 0x4b09e8: the won battle's 2.5 s hold with the experience cards |
| anim | `battle_pass` | 6 | 6 | both | 0x4afb54: a pass, 100 ms busy pointer |
| anim | `battle_slide` | 48 | 48 | both | 0x4afbd8: action sprite actor → target (counter: back) |
| anim | `camera_glide` | 12 | 12 | both | 0x4af96c: 900 ms glide (shown places, centre button) |
| anim | `card_slide` | 0 | 0 | never triggered yet | 0x4b0284: a battle move |
| anim | `look_at_army` | 2 | 2 | both | 0x4afa98: camera to a spell's target army |
| anim | `promotion` | 0 | 0 | never triggered yet | 0x4b1a04: the promotion screen |
| anim | `reveal` | 11 | 11 | both | 0x4af83c: the fog opening (map start, shown places) |
| anim | `unit_action` | 2 | 2 | both | 0x4b11cc: heal, potion or dismiss in a building |
| anim | `wait` | 12 | 12 | both | 0x4ae280: a wait, an event's delay, a spell's reading |
| anim | `walk` | 22 | 22 | both | 0x4ae6dc: the hero's walk |
| anim | `world_spell` | 2 | 2 | both | 0x4af2f8: a world spell's effect on an army |

## The gaps, and what was done

The gaps the first runs showed, by weight, and how Razdor plays them now (the interface in
`src/ui/`, the replay's log in `src/difftest.rs` and `src/av.rs` with it):

1. **Battle: the counterblow** (done). A counterblow or a preventive strike now adds the
   target's lunge back at the attacker and the action's own effect and sound on it (the
   action's half, then the echo's: `av::echo`, `battle_view::Fx`); a `DeathCurse`/`Ghost`
   death of the killer adds the sorcery effect and `Battle-Sorcery` on it (not reached by a
   run yet).
2. **Battle: the won battle's hold** (done). A win holds the battle screen 2.5 s with no
   input, the experience on the cards (a level gained shown there), the triumph starting at
   once; then the screen closes and the victory report follows on the map. Razdor's result
   box and its OK are gone for a win (kept for a defeat and for a battle nobody won). A
   level gained plays no `Unit-Upgrade` any more (the original's is the promotion screen's),
   after a battle or from an event. A player's pass is a 100 ms pause (`battle_pass`). The
   report comes 250 ms after the screen closes (the chained step 0x4af658, queued with the
   hold: `chain` in the trace, not compared).
3. **Building windows** (done). The building window opens with `InterfaceCastSpell` and
   every tab switch plays it; its Esc close is silent. The side windows open silent, their
   panel icon playing `InterfacePanelDown` (the bar's buttons played the button sound and
   the window `InterfacePanelDown` before). The replay presses the tabs the way the harness
   does (`original.py open_tab`: from the top until the tab shows), so the counts match.
4. **Trade, hire, heal, learn** (done). The money buttons (trade, hire, heal or raise, learn,
   a ship) play `Item-Gold` instead of the button sound; a hire slides the new card from the
   recruit into the army (200 ms) with `Card-Move`; a heal or raise plays `Battle-Cure` with
   the cure effect over the card (`unit_action`). The purchase's `Item-<type>` is gone, and so
   is the `Item-Gold` on any rise of the gold (midnight income, a sale's money). The market's
   list switch plays the button sound (as before). The original's two `Item-Gold` per hire
   are one sound restarted; Razdor plays it once (the replay logs both calls).
5. **Village tribute** (done). `Item-Gold` plays when the village window closes after the
   tribute was taken (its OK, Esc or a map click), with the window's button sound; not as it
   opens. The tribute itself is still taken on entering (the state's "window timing"
   difference, FINDINGS.md).
6. **World spells** (done). The camera glides (900 ms cosine) to the spell's army when it is
   more than 300 px away, before the reading for the hero's own army and after it for an
   enemy's, then the spell's effect plays over the army (`world_view::SpellFx`). The replay
   logs `look_at_army` always (the original queues it always; the glide it may add is not
   logged on either side unless far: no run had one).
7. **Shown places** (done). The places an event shows are flown to right after that event's
   window closes, then the camera flies back to the hero, and the next window waits for the
   flight (`world_view::Showing::free`, `App` holds the dialog); the shown place carries its
   event (`rules::game::Shown::event`). A map start opens the fog around the hero (`reveal`).
8. **Small ones** (done). The wait keys and the time panel play the button sound; the main
   menu rings `MainMenuSelect-1` as the pointer comes onto an item; the scenario row is
   silent, Next and Start play the button sound, a class portrait `MainMenuPress` only when it
   changes the class; an item plays its sound as it is taken from the pack (or a unit) and
   again where it goes, so wearing one plays it twice.

Not triggered yet (need runs that reach them): `Unit-Upgrade` (the original plays it only
in the promotion screen, 0x4b1af8; so does Razdor now), `Spell-Evil`, `Battle-Strike`,
`Battle-Sorcery`, `Battle-Cure` in battle, the item types other than amulet (a potion bought
no longer sounds), `BkgBattle2`, `BkgDefeat`, `BkgMenuMain`/`BkgAuthors`, the
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
the counterblow's lunge back and effect on the attacker included (gap 1; the DeathCurse or
Ghost effect on the killer is done but not reached by a run yet). The level-up has no
animation in the original either: during the 2.5 s hold after a win each card that gained
shows its experience and the promotion marker, which Razdor now does (gap 2), and
`Unit-Upgrade` plays only in the promotion screen. So note 3 is done as the original does
it; unit sprites are not part of it.
