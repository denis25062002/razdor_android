# Changelog

What changed in each version of Razdor. The newest version comes first; changes not released
yet are under "Unreleased". Each release lists the SHA-256 of its programs, which anyone
can rebuild from the same commit with `scripts/dist.sh` (see the README). From 0.1.2 the
release pipeline (`.github/workflows/release.yml`) builds and publishes a version when its
tag is pushed, and the SHA-256 are in the release's notes.

Each version names its commit. On 2026-09-30 the history was rewritten to a new author
email and every commit got a new id: the programs released before then show their commit's
old id, and their versions give both.

## Unreleased

### Changed
- **Random numbers as in the original**: one generator, the original's (the C runtime's
  `rand()`), started at 1 on every new map, so a fresh map's markets and every roll after
  them come out the same each time. It is no longer saved: loading a save starts it the way
  the original does (from the map's plants and its armies), so loading the same save twice
  replays the same rolls. Saves of the previous format still load.
- Midnight rolls building by building, each market's new goods before its barracks; a
  barracks slot always rolls, even when it is certain to grow.
- AI armies draw their four wander points as the original does: anywhere in their patrol
  box (or on the whole map), not only on cells they can walk to.
- The Community events' random flag (opcode 18) uses the Community's own generator,
  including its slip in the retry loop.
- Village offers roll as in the original: every roll is drawn until one is offered, also
  the one for the kind offered last time, and a visit with no offer lets that kind come
  back. The innkeeper and the priest compare with half the army rounded down, and the
  priest counts the living units, not only the wounded ones.
- A save load seeds the random numbers from the plants of the original's wider cell
  array (8 columns past the map's edge), as the original does.
- Battle magic as in the original: the target's protection rounds the power half to even,
  and a strike the protection takes to nothing still deals the GodAnger or GodStrike bonus.
  An undead caster's draining curse heals it by the whole amount, even when the target had
  less left.
- Vampirism heals only after melee and long strikes, never after a shot.
- Any army led by a Knight-type unit takes 80% physical damage, an AI lord's as well.
- Counter blows, preventive strikes, poison and bleeding no longer count as hit points lost
  for the battle XP.
- The space key in battle does what a click on the unit's own card does: one action, a
  spell on itself when it can cast one, else a pass. It no longer skips the whole turn.
- A front-row shooter or mage in the first or last column never sees a clear front, as in
  the original: the shooter hits only the enemies next to it, the mage cannot cast.
- A flying shooter or mage strikes in melee at the three front cells opposite instead of
  shooting or casting there. A Ghost casts at them whatever its magic direction, as long as
  the low byte of its power is positive.
- Rows collapse only after a death or a unit's last action, as in the original, never when
  a battle or a turn starts. When a battle starts with nobody in the player's front row, his
  back row moves up (and stays there); his reserve and the enemy do not move.
- The wide row's blocked cells move forward with a collapsing row, and a blocked cell spoils
  a "clear" front; the player's own battle grid has no blocked cells, as in the original.
- A turn starts unit by unit: each unit's turn-start bonuses come before its own
  regeneration or poison (Berserk reads the hit points it had before).
- The battle AI follows the original's scores to the letter, its slips included: the normal
  AI judges a kill by the hit points of its own unit at the target's place in the list, Life
  mages score curses with the original's defence mix-up, Elemental mages weigh a front-row
  haste by the blows needed to kill the enemies facing it, and a nearly dead Death mage with
  no spell for itself passes. The halving for a single action counts only for back-row mages,
  and a unit the AI counts as a warrior never steps back.
- The enemy is arranged anew for every battle as the original does it: its strongest front
  fighter in front, its shooters and mages behind, the rest by strength. Battles between AI
  armies arrange both sides, and the AI's practice battles play by the same rules (no
  Splash, the simple kill test).
- Unpaid units sit out only the battles their side starts: an army that attacks the player
  meets his whole army.
- After a battle the army keeps the cells its units ended on; units that did not fight, then
  the fallen, take free cells from the reserve forward.

## 0.2.2 — 2026-10-01

Commit `b01b492` (tag `v0.2.2`).

### New
- **Discord Times mods**: an install with a mod in it (its own `Rus_*.ini`, art and maps)
  plays with the mod's units, upgrade tree, items and spells, e.g. the Evolution mod (161
  units). The ini files are read as leniently as the original reads them: a value that
  cannot be read takes its default, an entry without a usable `GlobalIndex` (or an item
  without a `Type`) is skipped, and each case is written to `razdor.log`, as is an upgrade
  naming a unit that does not exist. Only a missing or unreadable file refuses the install.

### Fixed
- One value Razdor could not read made it drop the whole install and fall back to the demo
  with placeholder art. The Evolution mod's spell «Сангвинаре Вампирис» ends an effect line
  with an empty start time (`Effect2=…,1500,`), which now reads as 0.

SHA-256 of the released programs (built by the release pipeline):
```
994ecb4309cbfd932280b37fe4a3fd6bfe89cd0c84b605f207d11d434d0fc1d8  razdor
a6f42ab99ad4968b6af2181db53ea022a59260f2ce4b4d0147c8d0afad5ac4d7  Razdor.exe
0770c065199ea27449911994f331acca4452c679b6787a12b3c1e5757791a7cb  razdor-macos
```

## 0.2.1 — 2026-10-01

Commit `68cc569` (tag `v0.2.1`).

### New
- **A macOS program**, `razdor-macos`: one universal file for Apple Silicon and Intel Macs
  (macOS 11 and later), built by the release pipeline on a Mac (`scripts/dist-macos.sh`) and
  published with the Linux and Windows programs. It is not signed by a developer: lift the
  quarantine once (`xattr -d com.apple.quarantine razdor-macos`) after downloading it.
- The release pipeline can be run by hand, to build a commit's programs without publishing.

SHA-256 of the released programs (built by the release pipeline):
```
ce4e76a42cf85a37b16c638782013f0994fb686ae323c26c0a1a5d3d91883768  razdor
c63e6bd7544c836df4afc84ecdffb456fcf8c9ce77ee9194ebdc353b420198c6  Razdor.exe
ca3ce480dc84634988249aacad2c6ad73b8d10996c139230d1dba63753f739f3  razdor-macos
```

## 0.2.0 — 2026-09-30

Commit `610a898` (tag `v0.2.0`); its programs show `85b063f`, the id before the history
was rewritten.

### New
- **Following an army** as in the original («Автоматически преследовать выбранную армию»):
  after a click on an army (a second click, with the route preview) the hero keeps going to
  where it is now, until they meet; a right click or Space stops him.
- **Rearranging the army by dragging**: on the army screen and in the barracks a unit's card
  dragged onto another cell goes there, swapping with a unit standing in it.
- **The play log** (`razdor-play.log`, next to `razdor.log`): the session's games, screens,
  messages, walks, world and scenario events, and every battle in full with both armies'
  units, stats and items, for reading back when something plays wrong.
- **Route preview** (a Razdor extra): a click on the map shows the route and its travel
  time; clicking the same spot again, or a double click, walks it. A right click, Space or
  a move of the hero drops the preview.
- **A click while the hero walks stops him**, as in the original (a right click and Space
  still do too).
- **Armies on the minimap:** every army on explored ground shows as a mark in the original's
  colours (`ColorMarkEnemy` for hostile ones, `ColorMarkAlly` for the others), under the
  hero's blinking mark.
- **Drag the map with the right button** (a hand cursor while it moves); a right click that
  does not move still stops the walk.
- **Edge scrolling** as in the original: the mouse at an edge or a corner of the window pans
  the map; a click on the map or Tab brings the view back to the hero.
- **After a quest shows places on the map** the camera flies back to the hero by itself
  (a click or Tab still skips straight back).
- **The defeat screen** offers «Загрузить последнее сохранение» (the newest save, manual or
  automatic, of the same map) and «Начать карту заново» (the same map, the same hero),
  besides a new game.
- **A shown place opens like an iris:** a circle grows from its centre to its edges, with a
  soft rim, instead of the whole area fading in at once.

### Changed
- **A meeting's words come before the fight:** when an army comes at the hero with an
  event's message («Встреча с Блэки»), the message is read over the map and the battle opens
  after «ОК», instead of the battle opening under the message.
- **No ship hints** in the map's top left corner («Корабль ждёт…», «В море…»): the original
  has none.
- **Map zoom** follows the screen size: at zoom 1 the map shows as much ground as the
  original at its 1024×768 (about 32 cells across), with larger cells and figures on larger
  screens, instead of fixed 32 px cells that made everything small on big screens. The mouse
  wheel and +/- still zoom from there.
- **AI armies keep to the same roads as the hero:** their routes go around castles and
  forts that aren't their own or a friend's, ruins that aren't theirs, and any other building
  hostile to them. The building an army heads for (to take it, heal, hire…) and the one it
  stands in stay open. Before, armies walked straight through any building.

### Fixed
- **The invulnerable take 1 hit from any blow or shot:** units with «Неуязвимость» and
  ghosts («Яростный Дух») lose exactly 1 hit however hard they are hit, piercing blow
  («Проникающий Удар») or not. «Кара Господня» and «Гнев Господен» added their 10 or 20 on
  top of that 1.
- **Spell pictures in the book and the sanctuary** take each layer's `ColorC` away, as the
  ini says ("colour correction (-RGB)"): «Исцеление» is green, the lightnings purple and
  cyan. They were multiplied by it, which tinted every picture towards that colour.
- **A percent bonus to a protection adds its points:** 44% magic protection with +20% from a
  spell or a potion is 64%, not 55% (the rest of the way to 100 closed by a fifth), up to
  100%. The same for regeneration and vampirism.
- **Casting on the map takes its time in front of you:** the hero reads the spell while the
  clock runs, as a rest does (the time panel shows «Чтение: 2 час» counting down), the armies
  move meanwhile, and the spell lands at the end. Before, the whole reading passed in one
  frame and the clock only jumped. An enemy reaching the hero loses the spell; a message
  pauses the reading; walking, resting or Space drops it. Mana is only spent when the spell
  lands.
- **Battle cards show the actions left:** "Mnvr" on the card and in the panel counts down as
  the unit acts and shows extra actions (haste, a first-turn bonus) in blue and lost ones in
  red, refilled every turn. It used to show the unchanging stat.
- **An item that raises maximum HP brings the hit points with it:** a unit at 70/70 given
  +10 HP is 80/80, not 70/80 (from the pack, handed from another unit, or given by a quest).
  Hit points already lost stay lost: 60/70 becomes 70/80.
- **Objects at the edge of the dark** (trees, hills, bridges, buildings) are drawn and fade
  into it with the fog's soft edge, instead of vanishing while part of the edge still showed
  ground.
- **Messengers come to the hero:** a friendly army that hunts only the player (such as
  «Посыльный» on «Тихая пристань») stood still in its castle, because the AI let armies go
  for the player only to attack him. It now comes to meet him once he is within its view
  range, and the meeting's event runs.
- **A crash on the map** ("byte index 1 is not a char boundary … `Деревня`"): a building
  drawn without its picture showed the first letter of its type, cut as a byte, which broke
  on Russian names.
- **The sell shop** shows items the market does not buy (personal and quest items such as
  «Проклятые кости», a price of 1 or less) as «не продаётся», and «Продать» stays off for
  them; before they looked sellable and the button only said no.
- **Enemies' items in battle:** the unit panel showed worn items for the player's units only;
  an enemy's now show too (a Тень wearing «Проклятые кости» looked as if it wore nothing).
- **Past the map's right edge** a strip of half a cell showed terrain without fog; the view
  now ends at the map's edge, and anything beyond the map is black.
- **A shown place** no longer shows a faint ring before it fades in: the fog over it is kept
  exactly as it was, soft edges included, until the reveal.
- **Builds:** a local build could differ from the release pipeline's when the Rust source
  component was installed (its real paths went into the programs); `scripts/dist.sh` maps
  them back, and a local build of v0.1.2 gives the released `Razdor.exe` bit for bit.

SHA-256 of the released programs (built by the release pipeline):
```
066579d52a7d5862d766c64d50deb97d13547b2cb52fe25dc1d0457a17dcf989  razdor
2f77bab02e97413fce47bbf39ebb328d95a4c613e5c83541239f6e94ca44bd84  Razdor.exe
```

## 0.1.2 — 2026-09-30

Commit `bb09967` (tag `v0.1.2`); its programs show `378c873`, the id before the history
was rewritten.

### New
- **Battle AI, easy or expert:** the settings window has the original's «Улучшенный
  интеллект противника в битве». Expert lets the enemy count a unit as killable when the
  actions it has left can finish it, not only with one hit. Until changed it follows the
  install's own setting (`OptValue9`).

- **Quests show places on the map:** when an event lights a lantern or shows an army, the
  camera flies there once its message is read and the uncovered area fades in from the fog,
  one place after another. A click on the map or Tab skips it.
- **Unit cards in battle** show every gain or loss against the start of the battle (blue
  raised, red lowered), also the lasting ones, and the building's defence in the D values;
  the unit panel writes it apart, as the original does («15 + 12»).

### Changed
- **Ships** on the map are the original's: the hero's galley (at sea and waiting at the
  shore), pirate ships and merchant cogs, rowing and turning as they move. The drawn
  placeholder is left only for playing without an install.
- **The noon report** no longer opens when nothing came in or went out that day (no
  income, no wages, nobody unpaid or gone).
- **Item restrictions** follow the original: shields need a melee attack (warriors only),
  artillery cannot use bows, the undead cannot wear holy items («Святое писание», icons…),
  and «Королевская корона» is for the hero and a few noble units.
- **Building defence** also counts in a friendly building for the hero's side, and for an
  enemy army attacked in a building of its own side (before: the hero's own buildings and
  castle garrisons only).
- **Battle:** a side with nobody in the front row steps forward at once, also at the start
  of the battle (as the player sees in the original), not only after a death.
- **Map:** buildings are drawn in front of hills, rocks and trees, which no longer hide them.
- **Market:** after a buy or a sale the selection moves to the next item (or the one
  above), for many trades in a row.
- **The autosave before a battle** is the moment just before it: loading it puts the hero
  on the map next to the enemy, not straight into the fight.

### Fixed
- **Item and spell bonuses on protections, regeneration and vampirism** (`p-` values) did
  nothing for a unit starting at 0%: «Святое писание», «Меч "Кровопийца"», «Латы
  крестоносца», «Шлем Героя» and others now give their percent.
- **Esc in battle** opened the ways out and closed them in the same moment, and did nothing
  while an animation played.
- **Music after loading a game:** the triumph of a battle won before no longer carries on;
  the map music starts again.

## 0.1.1 — 2026-09-30

Commit `8d42bff` (tag `v0.1.1`); its programs show `48af7af`, the id before the history
was rewritten.

### New
- **New game:** the scenario list groups the maps as the original does. A campaign is one
  row under its name («Раменское королевство», «Сказка странствий»), with its chapters
  listed under it in play order; single scenarios are rows of their own. The list scrolls
  with the mouse wheel when it is longer than the window.
- **New game:** each map shows its own picture in the map frame, as in the original; the
  terrain preview moves to a small square next to the name, status and size.
- **The tutorial offer:** the first «Новая игра» opens «Обучающий сценарий», the original's
  window with its picture and text. «Да» starts the tutorial map and the hero choice, «Нет»
  opens the scenario list. It comes once (remembered in `settings.json`), and not at all when
  the install says the tutorial is done.
- **No OpenGL driver:** when Windows offers only its OpenGL 1.1 fallback (a Remote Desktop
  session, or no graphics driver, as in many virtual machines), Razdor explains what to do,
  in Russian and English, instead of showing the bare "WGL_ARB_pixel_format is required".

### Changed
- **Battle:** the enemy under the mouse is framed green, as in the original, not red.
- **World map:** the hero cannot walk through any army, friendly or hostile; he goes
  around it, or stops if there is no way. Before, only armies standing guard blocked him.

### Fixed
- **Menu:** the «Рестарт» question closed at once, because the click that opened it also
  answered «Нет». A question now takes clicks only from the frame after it opens.
- **Load window:** the same flaw in the «Удаление сохранения» question could delete a save
  with one click, without showing the question.
- **Builds:** the programs' SHA-256 depended on the folder they were built in (the order of
  the path remappings in `scripts/dist.sh`). The same commit and tools now give the same
  files anywhere.

SHA-256:
```
6f3291970860c208770d0af4d09e89d2b93d595fb84e22472fa80e4a30815540  razdor
4f39e09b7d4cd042cddc27314fd715be458d6c1a9104267b2f3cd44fb2cce3b0  Razdor.exe
```

## 0.1.0 — 2026-09-29

Commit `bf01d7a` (tag `v0.1.0`); its programs show `8593815`, the id before the history
was rewritten.

The first release: `Razdor.exe` (Windows x86_64) and `razdor` (Linux x86_64).

- Plays the original's scenarios from the player's own copy of *Discord Times*
  (Community Update 1.2): world map, buildings, armies, battles, economy, events and quests,
  spells, saves, sounds and music, with the original's art and texts read at runtime.
- The map editor, the built-in demo without an install, English and Russian interface.
- Army screen: items are dragged from the backpack onto a unit's card to give them to it,
  and between units.
- Settings: an optional FPS counter in the top right corner.
- Fixed a crash when the window is minimized or very small.
- The author's credit in the programs, the MIT license and the disclaimer.

SHA-256:
```
2863148726d8105bc7e14c6fdc377ec18f1db4c7546b2c4a9f5c313e3b0e9abe  razdor
b808bcb2d487f7cc8ea9c4d6618a9b86f91d1781f3dbcc9fc2bc57146fd1fdfe  Razdor.exe
```
