# Razdor

A small Rust prototype inspired by *Discord Times* («Времена раздора», Aterdux, 2004):
travel a kingdom map, hire a squad in towns, fight turn-based tactical battles on a grid.

```sh
cargo run --release
cargo test          # game rules
```

## How to play
- Pick a hero: Knight (melee; his army takes 10% less physical damage), Archmage
  (Elemental magic: slows or burns the enemy), Ranger (long bow; the army heals 20% a day).
- The demo kingdom is a hex map (`data/kingdom.txt`, one character per hex,
  odd rows shifted half a hex): click anywhere to walk the cheapest route. Roads are fast, forest and swamp slow, water and mountains
  impassable. Right click or Space stops.
- Time runs only while you travel or wait (**Wait 1 h / 4 h**, keys 1 and 4), as in the
  original. At noon the report window shows your gold and mana, the income of your
  buildings and the wages paid (from each unit's cost); units you can't pay refuse to fight
  and leave after a week unpaid. Villages refill their tribute at midnight.
- Stepping into a building opens its window, with the original's tabs: **Main hall**
  (description, quests and rumours), **Barracks** (hire from the building's stock, which
  regrows over the days; heal a wounded unit for part of its cost and an hour; raise the
  dead in towns and churches within a week, for three times its cost), **Garrison** (your
  castles and forts: leave units there, they are paid for their first day only and heal
  10% a day), **Market** (goods and a sell shop; prices rise when the building dislikes
  you), **Sanctuary** (learn spells into the hero's book of 15), and a village's
  **Tribute** (gold and mana, or the priest's healing, or the innkeeper paying off your
  unpaid men).
- Bandit gangs roam the map, chase you when you're close ("!") and attack on contact.
  Surviving camps send out new gangs every few days.
- Battles follow the original's rules (`docs/reference/mechanics.md`). Each side stands in a
  2×6 formation (the Community Update's wide row; the vanilla 3×4 with a reserve row is
  supported too). Before the fight, click a card and then a cell to move or swap it, then
  press **Fight!** (or Enter). The deployed formation is kept.
- Units act by initiative (the attacker gets +1). The green-framed card acts; it has as many
  actions as its `Mnvr` value, each spent on an attack, a spell or a step. Hover a framed
  card to preview the action ("strike: -12 hits", a curse's effect), left click to do it,
  right click for the alternative (a mage's strike instead of its curse). Click a lit cell
  to step there (columns c−1..c+1), Space ends the turn.
- Warriors fight only from the front row and hit the three enemy front cells opposite; with
  those three empty, a long strike reaches the nearest enemy front card, halving its
  defence. Shooters and mages in the back row reach anyone outside the reserve. Damage is
  attack minus defence, at least 1, no dice; the back row has +5 defence against shots.
  Mages strike, curse, heal or bless by their school; their power drains each turn.
- When a front row falls, the rear steps forward. There is no retreat; after 25 turns an
  undecided battle ends and both sides pull back. Your hero survives with 1 HP as long as
  anyone in his army does; you lose when the whole army is dead. Army cap: 12.
- Survivors gain XP ("XP +N" on the cards). Levels add stats; some units can be promoted
  from the **Squad** (hero and army) screen (the spearman becomes a swordsman at level 2).
  The fallen stay in the army as bodies until raised or buried; their items go to the
  backpack. A victory window shows the gold, mana and items taken, and any castle captured.
- Items, as in the original: every unit has 4 slots, one weapon, never two of the same
  type; melee weapons for warriors, bows for shooters, staffs for mages. Buy them at
  markets (new random goods every 7 days, sell for a quarter of the price), loot them from
  camps and gangs, or get them as village tribute. Manage gear from the **Squad** screen (4
  slots per unit, a scrolling backpack of 40); potions are drunk there (healing at once,
  other effects last until the end of the next battle). Units can be dismissed there.
- Units and items of the demo are our own content in `data/units.ini` and `data/items.ini`,
  written in the same format the engine reads from a Discord Times install.
- Clear both bandit camps to win. If your whole army falls, it's over.

## Using your Discord Times install
Razdor is becoming an engine for the original game's scenarios. It reads the data from **your
own installed copy** of *Discord Times* (Community Update) at runtime; the repo contains no
original maps, data, text or art, and nothing from your install is ever copied or written.

```sh
export RAZDOR_DT_DIR="/path/to/Discord Times"   # the folder with DiscordTimes.exe
cargo test                                      # also checks the readers against your files
```

What is read (only read, never modified): `Rus_Units.ini`, `Rus_Artefacts.ini`,
`Rus_Spells.ini`, `_Global.ini` and the scenario maps `Maps_Rus/*.DTm`. The readers live in
`src/dt/` (`dt::install::DtInstall::from_env()`); the formats are described in
`docs/reference/`. `rules::content::Content::from_dt` turns them into the definitions the
rules use. Without the variable everything still works, and the tests that need the
real files are skipped.

With the variable set, the first screen lists every map of your `Maps_Rus` with its title
and description (read from your files at runtime), after the built-in demo. Pick one, then
a hero class from the map's three presets. The world map uses the original's terrain
textures, objects, buildings and map figures; hover an army or a building for its tooltip
(formation, leader, owner, tribute), hover the ground for the route and its travel time,
mouse wheel or +/- to zoom. Hostile armies chase you and fight on contact, friendly ones
greet you; hostile castles, forts and ruins with a garrison fight when you step into their
gate, and a won castle or fort is yours with its income (an empty hostile one is taken by
walking in). Towns, castles, forts, churches, villages, markets and taverns open their
building windows with the stock, prices and spells of the map.

The map's events and quests run as in the original's editor manual: story windows (title,
text, picture, what you got, OK) open as time passes, when you step into a building or onto
an event point, after a battle, or when you meet an army on the road; the walk stops so you
can read them, and time stands still while one is open. Questions have **Yes** / **No**.
Accepted quests go into the **Journal** (bottom bar, key J) with their texts, and finished
ones are marked there; the main hall lists the building's quests and the rumours on offer
(10 gold each). The scenario's victory or defeat event ends the game. The fog of war hides what the hero
has not seen yet (unexplored ground is black and cannot be walked; a click into the dark
makes the hero feel his way towards it); M or the "Map (M)" button opens the minimap of the
explored land, and a click on it moves the camera. Ships are not in yet, so maps with
islands are only partly walkable. Russian text needs a TrueType font with Cyrillic: a common
system font is found automatically, or set `RAZDOR_FONT=/path/to/font.ttf` (without one,
names are transliterated).

## Spells
Learn spells for gold at a sanctuary (the **Sanctuary** tab of towns and churches; the book
holds 15). Open the spell book from the map with **Spells (B)** or B: every spell shows its
mana cost and casting time for your hero, how long it lasts and what it does. Blessings and
heals go on your own army; curses and bolts on a hostile army within 3 cells that you can
see. Casting costs mana **and game time**: armies move meanwhile, and an enemy reaching you
breaks the spell. The Archmage casts twice as fast for half the mana, a unit with the
Community `Caster` bonus takes another 20% off. Lasting spells change your units' (or the
cursed army's) stats in the battles while they last; the side panel and the book show the
time left. Healing and bolts act at once. The demo has five spells of its own
(`data/spells.ini`) at St. Beor's church and Greywall; its Archmage starts with two, and
villages pay mana. Scenario events that cast spells on your army use the same rules.

## Saves
**Save** and **Load** on the bottom bar, the **Menu (Esc)**, and **Load a game** on the
title screen. The load window has two tabs, your saves and the autosaves, newest first,
with the scenario, the hero and the in-game date. The game autosaves before every battle
and at every 12:00 report (named by the date, "1204.06.03, 12 h"); the newest 10 autosaves
are kept.

Saves are your data and live in your data folder, never in the repo or the game folder:
`$XDG_DATA_HOME/razdor/saves` (usually `~/.local/share/razdor/saves`) on Linux,
`~/Library/Application Support/razdor/saves` on macOS, `%APPDATA%\razdor\saves` on
Windows, or wherever `RAZDOR_SAVE_DIR` points. A save of a scenario stores the map's file
name and a hash of its bytes, not the map: loading reads the map again from
`RAZDOR_DT_DIR` and refuses if it is missing or has changed. Demo saves need no install.

## Sounds and music
With an install, Razdor plays the original's sounds and music, read at runtime from
`_Sounds.ini` and the `Sounds/` folder (nothing is copied; the `.raw` music is wrapped in a
WAV header in memory). The menu theme plays on the title, scenario and class screens; the
seven map themes rotate in random order on the world map and its windows; one of the two
battle themes in battle; the triumph piece after a won battle (then the map music again) and
at the scenario's victory; the defeat piece when the hero falls. Effects: buttons, windows
opening, the battle horn, melee, shots (cannon for shooters with ranged attack of at least
`ShotWeaponRange`), heals, blessings, curses and magic strikes, cards moving, event chords,
level-ups and promotions, casting a spell (good or evil by the target), items bought,
equipped or drunk (by type) and gold coming in.

- **N** turns the music off and on (anywhere except while typing a save name).
- The **Esc menu** has music and sound volume (**−** / **+**, keys **+** / **−** for the
  music) and **Off** / **On** for each. They are kept in `audio.json` in the save folder.
- The `.raw` files do not store their sample rate; Razdor plays them at 22050 Hz. If the
  music sounds too low or slow, try `RAZDOR_MUSIC_RATE=44100`.
- `RAZDOR_NO_AUDIO=1` turns sound off; `RAZDOR_AUDIO_LOG=1` prints each sound as it plays.
  The demo (no install) is silent.
- On Linux the sound goes through ALSA (`libasound.so.2`, present on any desktop; PipeWire
  and PulseAudio provide the `default` device). `cargo build --no-default-features` builds
  without sound.

## Custom sprites
All art is placeholder tokens. To use your own, put PNGs named after the units' and items'
`Key=` in `data/units.ini` / `data/items.ini` (`knight.png`, `archmage.png`, `ranger.png`,
`spearman.png`, `archer.png`, `swordsman.png`, `healer.png`, `bandit.png`,
`bandit_archer.png`, `bandit_chief.png`, `short_sword.png`, …) in a folder and run:

```sh
RAZDOR_ASSETS=./assets-local cargo run --release
```

`assets-local/` is git-ignored — keep third-party art there.

## Layout
- `src/dt/` — readers for the original's files (ini data, `.DTm` maps). Pure, no macroquad.
- `src/rules/` — pure game logic (no macroquad), unit-tested.
- `src/ui/` — macroquad screens; `assets.rs` is the only place that draws units and items.
- Design: `docs/superpowers/specs/2026-09-24-razdor-prototype-design.md`.
