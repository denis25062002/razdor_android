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
- The invulnerable and ghosts still take 1 from any blow or shot, but GodAnger and GodStrike
  add their 10 or 20 on top of it, as in the original. A unit with no attack of a kind
  strikes back with its attack modifier (a blessing's attack counts), and a counter blow or
  a preventive strike that kills the attacker sets off no death curse.
- A caster in the reserve can heal and bless a reserve unit that a NoHeal weapon marked.
- Any army led by a Knight-type unit takes 80% physical damage, an AI lord's as well.
- Counter blows, preventive strikes, poison and bleeding no longer count as hit points lost
  for the battle XP.
- A side left with only surrender-capable units gives up even when its last action wins the
  battle: a player whose hero has fallen and whose priests or mages kill the last enemy
  surrenders and loses, as in the original.
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
- The turn-1 initiative of Artillery and FirstShot units is part of their initiative, not a
  modifier: an Elemental mage of the AI may haste such a unit on turn 1, and the panel no
  longer lists it under "this turn".
- The battle AI follows the original's scores to the letter, its slips included: the normal
  AI judges a kill by the hit points of its own unit at the target's place in the list, Life
  mages score curses with the original's defence mix-up, Elemental mages weigh a front-row
  haste by the blows needed to kill the enemies facing it, and a nearly dead Death mage with
  no spell for itself passes. The halving for a single action counts only for back-row mages,
  and a unit the AI counts as a warrior never steps back.
- The battle AI moves as the original's: units in the reserve only move (or tend the
  reserve), a front-row unit may keep its cell when that is where it scores best, and a unit
  with nothing to do casts on itself when its own cell offers a spell.
- A front-row caster of the battle AI with nothing better to do may heal or bless an ally in
  the front row, in any column, when that ally's cell scores best among its moves. A unit
  that wants to step back but finds no free cell behind it may instead step to the edge of
  the front row (or spend the action) when an enemy stands at the far edge, as in the
  original.
- The enemy is arranged anew for every battle as the original does it: its strongest front
  fighter in front, its shooters and mages behind, the rest by strength. Battles between AI
  armies arrange both sides, and the AI's practice battles play by the same rules (no
  Splash, the simple kill test).
- Unpaid units sit out only the battles their side starts: an army that attacks the player
  meets his whole army.
- After a battle the army keeps the cells its units ended on; units that did not fight, then
  the fallen, take free cells from the reserve forward.
- Those units fill each row from its first column, as the original's write-back does, not
  from the centre as a new unit does. A fallen or unpaid unit standing in the front row
  keeps the back row from stepping up when the next battle starts.
- Before every battle the original plays it once in secret, the AI on both sides, and the
  losses it predicts feed the battle XP; Razdor does the same now, so the XP pool follows
  the original's formula with the predicted loss and the worst turn's loss.
- A new unit (hired, given by an event, or at the start of a map, for the AI too) takes the
  first free cell of the reserve, then of the back row, then of the front row, whatever it
  is, as in the original.
- Community Splash as in the patch: the 80% malus counts in every battle and in the AI's
  estimates, and the 40% loses one on a multiple of 5 (an attack of 10 gives 3). Each
  neighbour gets the whole action again, in the order the units stand in their army: its
  preventive strike and counter blow (at 40%), vampirism and the rest. The primary target
  also strikes back at 40%, a splash heal reaches a NoHeal-marked neighbour, and off screen
  heals and blessings still splash.
- No preventive strike before a spell, only before blows and shots.
- Stun takes the same 30% of the target's initiative with every hit; ArmorBreaker leaves
  `x − x/4` of each defence (5 → 4, 1 → 1); FateGift saves a unit from a Neutralize blow;
  Berserk is recomputed before Drying's loss; BloodThrist counts every killing hit and not a
  target that fate saves.
- A Poison mage poisons when its own power, cut by the target's protection as the patch
  computes it (`× (99 − protection) / 100`, Elemental `/ 114`), is above 15, whatever Splash
  or Potent do to the spell.
- Assault's ×2/3 follows the patch's test of the attacker's building and initiative: from a
  building of 1 to 127, or in the open while slowed (a negative initiative modifier).
- A Suicide unit is not removed by its own blow: at 0 HP, with no actions left, it keeps its
  side in the battle until a counter blow or the next turn start removes it, and its
  vampirism can give it hit points back meanwhile.
- The magic drain and floor are per unit type: a type without magic power of its own neither
  drains nor floors, and an undead Death type with its own `MinMagicPower` gets no +25.
- NoHeal's mark stays on the place in the army's list, so a death before the marked unit
  passes the mark to the next one, as in the original.
- Flock compares the two sides as they stood after the last action of the battle on screen
  (battles between AI armies see that battle's counts too), and Hunger's count of removals is
  shared by every battle and kept from one battle to the next.
- Bleeding and Flock divide unsigned as the patch does, Evasion is read as a byte, and the
  player's twelfth unit's death stops the enemy's first unit bleeding, all as in the patch.
- An EternalGift change to a unit's initiative moves it in the turn order only from the next
  turn, and Stun keeps taking 30% of the initiative the unit started the turn with. An
  EternalGift blessing or curse on a unit cursed below 0 attack still changes that attack,
  not its shot.
- The battle AI scores a melee target with 0 Manevres with the patch's huge constant, its
  32-bit wrap included, so it fixates on such a target or ignores it as the original does.
- **The hero's route is planned as the original plans it**: a flood from the clicked cell
  that prices each step by the cell he leaves and stops as soon as it reaches him, so a
  route can be a little dearer than the cheapest (a diagonal first step, say). The walk goes
  to the very cell clicked, also inside a building.
- A click on an unexplored cell does nothing: the hero no longer feels his way into the dark.
  Water is a target only with a ship; a click next to open ground no longer means it.
- The route goes around only castles and forts whose attitude is 0 or less and ruins not
  his; every other building, ill-disposed towns included, is crossed. At sea, bridges close
  only when he clicks land or stands on one.
- Stepping onto an army engages it, before he moves: a hostile one fights, a friend meets
  him. Stepping onto a cell of a village, castle, fort, ruins or bridge meets the army that
  lives there, or the garrison of an ill-disposed castle or fort (attitude 0 included) at its
  gate; an empty one is taken, and so is every unguarded village stepped on, even when the
  route only crosses it.
- A building is entered on its second cell crossed (its events may fire) or where the walk
  ends; its window opens only there.
- AI armies attack or greet the hero only right after a step of his, never while he waits or
  casts, and never step onto his cells: they stop next to him. A friendly army greets him
  when its talk counter is above 0 (it grows as the army steps), then not for a long while.
- AI armies pay for a step with the cell they leave; stationary guards bank no time.
- A pursued army that goes out of reach ends the pursuit and the hero stops.
- An event that fires when the hero steps onto an army takes the place of the battle: the
  army then leaves him alone for a while. Going to sea or ashore makes the AI armies on that
  side lose their banked time and plan again.
- **Ships as in the original**: buying one puts no ship on the water; he steps out of the
  shipyard onto the water to sail, and leaving it on foot loses the purchase. Landing parks
  the ship on the water he left; the original's landing test, which reads a cell further
  south, is kept, so he sometimes stops on open water or steps ashore and loses the ship.
- A plant, mountain or rock standing in the water blocks ships; overlapping hills are laid
  in the original's row-by-row order.
- The hero's sight, speed and casting time stay those of the class he started with, whatever
  unit an event makes him; a Community speed event sets his speed.
- Sight and lanterns explore exactly the original's cells (its soft half-cell stamp: the
  archmage's 8 cells reach 8 more cells than before). A lantern without a radius lights
  nothing.
- The clock starts a minute after the map's start time, and the hero's first noon report is
  always the next day's, even after a morning start.
- The noon report comes in the first event check after 12:00 in which no event fired and no
  spell is being read (so after a cast, not in the middle of it); midnight comes after the
  armies have moved. A noon held up past midnight skips that day's own noon, as in the
  original.
- A friendly army's talk counter grows with each of its steps wherever the hero is, so it
  greets him again sooner. A greeting no longer stops his walk unless one of its events
  fires; an AI attack whose events fire brings no battle. Of several armies next to him,
  the last in the map's order acts.
- After a walk, AI armies keep off the cell in front of the hero (his last step's
  direction) while he stands, as in the original. A pursued army that goes into the dark
  ends the pursuit.
- An event that moves an army to the hero puts it on his cheapest free neighbour (a road
  before grass, a building only as a last resort), moves its home there too but keeps its
  patrol area where it was, and leaves a waiting army off the map.
- **The world-map AI as in the original**, its slips included. AI armies keep no goal: at
  every step they may plan again (every few steps, or every step with anyone near), with
  one flood from everything they want at once, and walk the way it gives. They score other
  armies and the hero by a battle played in secret (aggression shifting it, a hostile one
  worth more, a lost one a danger they route around), and every building by its village
  gold, what they could buy, whether they can take it and its garrison; a stationary guard
  they cannot beat closes a building to them.
- Stationary guards no longer move, plan or get paid; an army with nothing to do steps in
  place, and greets a friend or the hero standing next to it.
- AI armies greet each other (and, after his step, the hero) by talk counters, and attack a
  hostile neighbour only when their battle score says so; an enemy sheltering in a third
  party's building is not attacked.
- In a building an AI army assaults it if hostile (a tavern or church on its way too, a town
  only at its worst attitude), takes villages, castles and forts it wins, makes altars and
  ruins neutral, collects any village's gold (feudal lords), sells its pack and buys items
  by what they add to its units' strength, heals by its units' hit points left, raises its
  dead in towns and churches, hires by battle role and its leader's Nature, and buys and
  deals out its own castles' garrisons.
- AI armies keep their units' worn items, their dead (raised or dropped after a week) and
  their pay; a feudal lord short of gold at its noon leaves its cheapest units unpaid, and
  they stay out of the battles it starts. An army's noon comes at its first step after
  12:00, from its income, its castles' stock and its villages.
- Battles between AI armies: the loser's wage bill and gold go to the winner as in the
  original, the loot of items to the side with more hit points left, worn by whoever they
  help most; the AI's units are promoted by its own rolls, their items to the loot.
- Beaten armies no longer retreat into their castle: they respawn after their days, whole
  when an AI army beat them, the leader alone when the player did (unless the map says
  whole); a rogue respawning at its ruins takes them over.
- Armies placed on water are ships and plan like any army on the sea; ships with no patrol
  of their own wander the whole sea.
- Units with two upgrade options keep them in the first and third slot, as the original's
  loader moves them.
- Saves of the previous format still load; their AI armies start their plans afresh.
- An AI army buys a good of negative price (a personal item) for its absolute price.
- The AI rescores its battles against a feudal army after that army's noon, and against the
  hero after his noon, an event that took effect and a visit to a building, as the original.
- An army placed on water inside a building's footprint other than a bridge is a ship.
- An AI army standing still that the hero's cell bars counts an idle plan or a step as the
  original's path index says (an army that planned nothing in mid-path, or has no path at
  all, counts it idle).
- An army an event brings onto the map comes back with its dead raised and everyone paid,
  takes its place among the armies in their order (it moves in its turn, not last), draws
  its wander points, gets its home back when it stands in it, and the AI rescores it.
- AI hiring scans a building's six barracks slots with their empty ones, as the original:
  a unit hired from the sixth slot moves the scan on to the next role, which can lower the
  cap to 8 units early; a slot's stock is the one that goes down.
- An AI unit raised again (a leader left at 1 hit point, a resurrection) keeps its first time
  of death, as in the original: if it falls again, its corpse is dropped that much sooner.
- An AI army beaten in a fight of its own step goes on with the rest of that step as the
  original's record does: it may attack the next enemy with nobody and lose again, take a
  village's gold, buy, heal or hire, and it comes back with all that when it respawns.
- A beaten army that comes back (by its respawn or an event) is no longer "beaten" for the
  events, and an army beaten by the player and then by an AI army counts as beaten by that
  army only, as the original keeps one mark. An event can bring back a beaten army, even
  one that would never respawn, and an event that removes a beaten army stops its respawn.
- An army that respawns or that an event brings back takes its first step at once, for
  free, as in the original.
- **The noon payment as in the original.** Castles and forts pay the gold stock they have
  grown since the last noon (×F/100), not a fixed income, and towns pay nothing; every
  building with a maximum grows its stock at midnight. No building pays mana at noon, and
  villages linked to the player's buildings give him their gold only.
- Corpses draw no wage. Rear Service cuts the whole wage bill once at noon (by the player's
  stored income, also for the AI's armies), and the wages shown are the full bill. A short
  noon refunds full wages, cheapest first, a corpse's too, never an elemental's; deserters
  leave with their worn items.
- With no mana at a noon (any army's), the Community's mana-short flag goes up and stays up
  until a short-gold noon: meanwhile a unit left unpaid stays unpaid though its wage is
  paid, as in the original.
- The noon report shows the nominal income of the player's towns, castles and forts, the
  bill and the gold before the payment, warns when they do not cover the wages, and is not
  shown when there are neither wages nor income. A Ranger heals 20% more when it is shown;
  a dead Medic still heals at midnight. Saves of the previous format still load.
- Building tabs as in the original, with no attitude test: any building with a barracks
  unit hires and heals (a tavern or altar too, an ill-disposed one too), unless a barracks
  unit is not of ordinary Nature and the building lacks the "all types" flag; castles with
  no barracks no longer heal. Only towns, markets and churches sell items; the obelisk has
  no window. An ill-disposed village pays its tribute.
- The player's dead are never buried by time: they can be raised in a town or church any
  time, and come back paid. A unit whose Cost is 2 more than a multiple of 256 is raised
  for mana after a gold check, the Community's slip; an elemental's healing is checked
  against the gold and paid in mana.
- Ruins keep only their first five goods as treasure.

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
