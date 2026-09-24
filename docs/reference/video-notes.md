# Discord Times 1.5 — gameplay video study (Раменское королевство, "Столица" part 1)

Source: 40-min recording (960×720). Timestamps are video time (mm:ss). About 350 frames reviewed
(10 s contact sheets over the whole video, 1–2 s sheets and full-res frames for the key spans,
bottom-bar crops to follow money, time and income). Everything below is what the video shows.
Where a value is my reading of the screen rather than something stated on it, it is marked *(inferred)*.

Hero in the video: **Архимаг Stings** (Archmage, level 5, XP 500/590). Squad: Кирасир → Рыцарь (knight),
Волшебница (sorceress), Святой брат / Ополченцы (militia) later. Game date runs 1204-05-19 → 1204-06-03.

---

## 1. World map

**View.** Pre-rendered isometric-style terrain with free (non-grid) movement. There is no visible hex
or square grid. Click a spot and the hero walks there; the planned path shows as small white arrow
marks on the ground (14:40, 20:00). While you hover a location the cursor turns into a "?" (06:46).

**Fog of war.**
- The hero reveals a soft-edged, roughly elliptical area around himself. The radius is about 250–300 px
  at 960×720, about ¼ of the screen width (06:46, 37:02).
- **Explored terrain stays visible.** Places you have visited stay fully drawn and do not grey out
  again (18:30, 20:00: long revealed corridors with black unexplored blobs between them). There is no
  "seen but not currently visible" shading. Enemy armies inside explored terrain still show and move.
- The camera can scroll into unexplored black (06:40–07:00, the player clicks the minimap far away and
  sees only black).

**Minimap.** A toggle window in the top-right corner, about 400×400 px, with a stone frame. Its button
is the orange spiral, the right-most bottom-bar button, which glows while the map is open. It shows only
explored terrain; the rest is black. It draws coloured icons for locations: castles/forts in white, blue,
red, green or yellow (the owner's colour *(inferred)*), village house icons, and a light rectangle for the
current viewport. Clicking it moves the camera (06:40, 14:10, 16:40, 32:10).

**Time display.** Centre of the bottom bar, parchment panel:
`Время:` / `1204 год, 5 месяц, 20 день, 11 час`. The clock has hours only, no minutes.
While a move is in progress a second line appears: `До конца пути: 2 час` / `меньше часа`
(time left on the path).
- **Days are 0-based and months have 30 days:** `5 месяц, 29 день` is followed by `6 месяц, 0 день`
  (19:40).
- **Time passes only while the hero moves** (the clock stays put while he stands, 18:40–18:45).
  Walking is fast: about 24 game hours in about 15 real seconds (18:18–18:35).
- Casting a spell on the map costs game time (`Чтение: 2 час`, reading time).

**Bottom bar** (the whole width, about 80 px tall):
- Row 1: four round buttons on the left: ✕ exit/menu, gears (settings), disk-in (save), disk-out (load).
  Then the time panel. Then four on the right: flag (journal/quests *(inferred)*), crossed swords (hero and
  army screen), book (spellbook), spiral (minimap). The buttons go grey while a modal window is open.
- Row 2 (thin text strip): `магия` + blue hand icon + mana (e.g. 1172); `деньги` + coins + gold
  (e.g. 4894); `доход` + house icon + `+ 155` (daily income); `выплата` + knight icon + `- 330` (daily wages).

**Hover tooltips** (translucent green-marble panels next to the object):
- **Army** (14:10 "Отряд фон Эркхила", 17:40 "Вампиры из развалин", 25:40 "Армия фон Моргена"):
  a title, a **2×6 thumbnail grid of the army's formation** (empty cells show the empty-slot icons),
  `Предводитель` + the leader's name in green, and a flavour paragraph.
- **Castle/fort** (28:20 "Замок Макарат"): a title, `Владелец` + owner, a description,
   + a 2×6 portrait grid.
- **Village** (10:37 "Деревня Кузнечное"): a title, `Владелец` + headman (`Староста Мелин`),
  a description, and a status line in orange: `(дань уже собрана)`, tribute already collected.
- **Market** (14:50 "Грэхилский базар"): owner and description.
- **Landmark** (15:10 "Древняя Руина"): name and a short description only.

**What triggers battles.** Entering a hostile fort or castle (garrison battle, e.g. "Форт в Трясине"
09:40, "Замок Айлхолд" 13:00). Walking into a roaming enemy party ("Шайка Арчи", "Грэхилские мародеры").
Enemy lord armies can also move onto the hero themselves (18:04–18:08: "Армия фон Локотуна" catches the
hero on the road). A party that is not hostile can greet you instead: the dialog "Встреча на дороге"
(32:20) shows only an OK button and starts no battle.

**Roads, bridges, water.** Dirt roads, stone bridges over a river (the "Северный мост", Northern bridge,
is named in a quest). The hero crosses the river on a **boat** (14:20 and 17:00: a sailing boat icon on
the river with the hero aboard; the boat moves along the water).

---

## 2. Towns, forts, villages

### Town window (06:09 "Город Раменбург", 19:40 "Город Кроссбург")
A 840×600 modal window titled `Город <name>`, with a close ✕.
- **Left column:** parchment tabs `Главный зал`, `Казарма`, `Рынок`, `Святилище`, and `Выход` at the
  bottom. The active tab has a silver frame and red text. A small market ("Грэхилский базар") has only
  `Главный зал`, `Рынок` and `Выход`, so a location may have any subset of these buildings.
- **Right side:** the content pane at the top, then a thin divider, then a flavour text box describing
  the town.

**Главный зал (main hall).** A town picture. A list  (offered quests and
rumours) with a scroll bar and a `Взять задание` button. A list entry such as `Слухи в таверне` opens a
Yes/No dialog: "buy wine for 10 gold and try to eavesdrop?". Yes → a rumour text (a hint about treasure
near a village) + `Деньги - 10` (06:12–06:40). **The tavern is a rumour entry in the main hall, not a
separate tab.**

**Казарма (barracks)** (19:50, ref 03):
- Top: **6 recruit portraits** with a green `Нанять` button and `Цена = N` under each. The button turns
  grey when you cannot hire that unit. Prices seen in Кроссбург: 50, 90, 130, 170, 55, 35.
- Under the portraits: three counters, `Деньги 5303`, `Выплата армии - 330`, `Доход + 155`.
- Bottom: **your 2×6 army grid.** Each wounded unit has a blue `Лечить` button and `Цена = 26 / 27 / 65`
  (paid healing, priced per unit). Army cards can be rearranged here too.

**Рынок (market)** (ref 04):
- A list with columns `Название` / `Цена`, an item icon on each row, and a scroll bar.
- `Описание предмета` pane: a large icon, the name, a restriction line (`только для воинов`, warriors
  only), flavour text, and bonuses in blue (e.g. ).
- Buttons: `Инвентарь`, `Деньги 5408`, `Купить`, `Торговая лавка` (the selling shop).
- **Stock and prices differ per market.** Раменбург: Медное колечко 49, Круглый щит 70, Кожаный доспех
  147, Кожаный панцирь 329, Меч Рыцаря 700, Лук снайпера 1050, Амулет "Robus" 1645, and potions
  (Лечебное зелье, Зелье скорости 176, …). Грэхил: Кинжал "Жало" 122, Топор Конунга 203, Кожаный доспех
  **213**, Кольцо защиты 305, Кольцо "Орлиный глаз" 406, Часы времени 508, Кольцо "Глаз дракона" 1320.
  Кроссбург: Вилы 35, Круглый щит 70, Охотничий лук 105, Секира "Кровопийца" 196, Настойка св.Георгия
  245, Кровавый крест 1190 (), Кольцо вампира 1890.
  Item bonuses can be flat or %, and there are effects such as vampirism.
- One hero trait changes prices: `Торговец-Эксперт` gives +50 % when selling and −30 % when buying.

**Святилище (sanctuary) = spell shop** (07:11, ref 05):
- List `Название заклятия` / `Цена
  Слабость 240, Ядовитый Туман 200, Ветхость Доспехов 220.
- The `Описание заклятия` pane shows the effect, the mana cost, the reading (cast) time and the duration.
  Example: .
- A spell you already own shows .
- A spell is bought once and goes into the book; mana is then spent per cast.

### Fort/castle you own (29:20 "Горные врата", 21:28 "Старая башня")
The same window style with a portrait on the left and the fort picture on top, the counters
Деньги / Выплата армии / Доход, and your army grid. You can hire the garrison-type unit (a militia,
`Ополченец`, appears with its stat block) and swap units.

### Capturing
Beating a fort's garrison opens the victory window (09:46, 13:07): an icon, a message that you captured an ownerless fort, a line saying you won gold from the enemy, and sometimes a line saying the surrendered enemy troops pray for you. Rewards:
`Деньги + 30`, `Магия + 20`, or `Деньги + 125`. **Daily income goes up at once:** after "Форт в Трясине"
it rose from +125 to +155 (09:44 → 09:46), later to +175.

### Villages
Visiting a village collects tribute (`дань`). The hover tooltip then says `(дань уже собрана)` until it
resets. Villages have a headman (`Староста`) as owner. The narrative calls some "church villages" that
feudal lords compete for, which suggests village ownership can change *(inferred)*.

---

## 3. Battle screen (ref 09)

A modal window about 840×600 over the map, titled `Сражаются: армия героя Stings и <enemy name>!`.

**Layout.**
- **Left panel (about 245 px wide, parchment):** a full-body portrait of the **currently active unit**
  (or the hovered one), its name, then the stat list, then its trait descriptions with small icons.
  A unit's equipped items show as small icons at the portrait's top corners.
- **Right side: two 2×6 grids.** The **enemy is on top** (rows 1–2) and **the player is at the bottom**
  (rows 3–4). Between them a red banner reads .
  The enemy's front row is its lower row and the player's front row is its upper row, so the two
  front lines face each other in the middle.
- **Empty-cell icons:** a tent for the outer columns (0 and 5) of both rows; crossed swords for the
  middle four cells of the front row; a bow-on-shield for the back row's inner cells. These are hints
  for where each unit type belongs.

**Card (about 88×120).** A portrait with a stat strip underneath:
- warrior: `A: 45  D: 35/40` / `Mnvr: 1  Ini: 12` / `Hits: 70`
- caster or healer: `Pwr: 58  D: 10/18` / `Mnvr: 2  Ini: 29` / `Hits: 77`
- A = melee attack, Pwr = magic power, D = melee/ranged defence, Mnvr = actions, Ini = initiative.
  When wounded, Hits shows `78/90`. A value in red means it is currently lowered (e.g. `Mnvr: 0` in red
  on a cursed enemy, 29:40).
- Small badges sit on the card corners: an orange round badge at top-right on some units (a buff or
  level mark), a blue shield at top-left (defence bonus or garrison, "в собственном строении"), and a
  skull on a unit that is about to act or is targeted *(inferred)*.

**Stat names in the left panel** (as written):
`Уровень`, `Опыт x / y`, `Жизнь (хиты)`, `Атака рукопашная`, `Атака стрелковая`, `Защита рукопашная`
(often `0 + 25`: base + bonus), `Защита стрелковая`, `Магический удар (- хиты)`, `Лечит (+ хиты)`,
, `Ускоряет (+ действия)`, `Уменьшает инициативу`, `Замедляет (- действия)`,
`Защита от магии жизни` %, `Защита от магии стихий` %, `Защита от магии смерти` %, `Регенерация` %,
`Инициатива`, `Количество действий`, `Ежедневная выплата` (daily wage).
- A stat line is blue when a bonus modifies it, red when a debuff lowers it, and orange for the wage.
- Traits (icon + text) seen:
  - Кара Господня: +10 damage beyond the attack, ignoring all defences.
  - Personage in the second row gets a ranged-defence bonus.
  - Personage inside his own building gets a bonus to all defences.
  - Архимаг: casts twice as fast for 50 % less mana, but the army gets no bonuses.
  - Торговец-Эксперт (trade bonus).
  - Быстрая атака (Конный сержант): acts first.
  - Лучник: a promotion after N successful battles.

**Turn flow.**
- Units act one at a time in initiative order. The left panel switches to show each actor.
- The active unit gets a **green frame**. The player's empty cells it can move to get **blue frames**.
- On valid targets a hover tooltip previews the action:
  -  / `Урон -67 хитов` (expected damage);
  -  / `Инициатива: -5 Действия: -1` (a debuff spell's effect);
  -  (select an ally).
- Hovered or targeted cells get a red tint. A cell that takes damage flashes red or fire-orange.
- Healing and buffs show as a blue/white star (`✦`) on the target. A curse shows purple sparkles.
- Enemy casters use area fire effects: 18:10 shows a large fire ring centred on one player card that
  covers it and its neighbours *(the exact cell area is not certain)*.
- **Blessing** (`Благословение`: physical attack +20 %, magic power +20 %, mana 260, reading 2 h,
  duration in hours) is cast on the map before the battle and is visible as a whole-army buff.

**End of battle.**
- **XP floats on every surviving player card** as a cyan badge `Опыт +24`, `Опыт +26` (09:44, 15:40).
  They appear on the battle screen just before it closes.
- Then the `Победа над врагом!` window with rewards: `Деньги + N` and `Магия + N` icons, and an OK button.
- **ESC during a battle** opens : "you cannot return to the map without
  finishing the battle". Buttons: `Выйти из игры`, `Выйти в меню`, `Рестарт`, `Отмена`. There is no
  retreat; to get out you reload.
- On a defeat the player simply reloads the pre-battle autosave (18:14, 26:00).

---

## 4. Hero / army screen (, ref 11, 12)

- **Left:** the portrait of the selected unit with **equipment slots around it**: two at top-left
  (e.g. a heraldic shield badge, a potion flask) and two at top-right (weapon, armour or cup). That is
  4 slots, matching the fan wiki. Below them: the name, the full stat list (the same names as in battle)
  and the traits.
- **Middle top**, one of two views:
  -  (unit upgrade tree): the current class at the bottom, arrows up to 1–3
    promotion classes (Кирасир → Рыцарь; Ополченец → two options). Used when a unit levels up
    (Кирасир level 2, XP 33/560, 14:00: its tree becomes active and it later appears as `Рыцарь`).
  - The **hero's inventory**: a **5-column grid** with a scroll bar, about 5 rows visible (25+ items:
    potions, rings, weapons, relics, beer mugs…).
- **Right top:** `Описание артефакта` (item description on hover). For a recruit there is also a
  small portrait and a red `Прогнать` button (dismiss the unit).
- **Bottom:** the **2×6 army grid**, the same as in battle. Click a unit to select it and drag or click
  to rearrange. You can cast healing on the army from here: green sparkles on the target, Hits 78/90,
  mana drops (10:50).
- **Items have class restrictions** (`только для воинов`). Items give flat or % stats.

---

## 5. Quests, dialogs, reports

- **Story/quest dialog:** a green-marble window with a title bar (e.g. `Королевская награда`,
  , `Поручение барона`, `Отчет о Мародерах`), a red-brown parchment text box,
  optional reward icons (`Деньги + 1000`, `Получены предметы:` + an item icon such as a golden cup),
  and a single `Ок`.
  - Accepting a quest adds the blue line  (a journal entry
    was added).
  - Finishing one gives  and the reward (`Деньги + 50`)
    (18:40).
  - The quest giver is a lord or baron; you report back by meeting his army or visiting his castle.
- Arrival dialogs can show `Ушедшие персонажи:` with a portrait: a companion leaves the party (00:30).
- **Resource report**  (07:15, 18:30, 37:04): a short explanation
  and four icons with . It **appears once per
  day when the clock reaches 12 час.** Money changes at that moment (5458 → 5253 on day 29, 12:00, which
  is net income minus wages plus a small extra *(inferred)*).

---

## 6. Save / load (ref 14)

 window over the title art:
- Two columns, `Имя сохраненной игры` and `Название сценария`. Each row shows the scenario (`Столица`)
  and the real-world save time (`Время: 2023 год, 1 месяц, 31 день, 13:08`).
- Tabs `Личные` (manual) and `Авто-сохр.` (autosaves).
- **Autosaves are made before every battle** (`Битва - Замок Макарат`, `Битва - Армия фон Моргена`,
  , …) **and every day at 12:00** (`1204.06.03, 12 час`).
- Buttons `Загрузить` / `Отмена`. An arrow marks the selected row, and a red skull icon appears on some
  entries (hard or last battle? *(unclear)*).

---

## 7. Other mechanics seen

- **Mana ("магия") is a party resource.** Battle victories and captures raise it (+20). It also rises
  over time or at locations (1212 → 1382 while travelling, 10:10 *(source unclear)*). Spells cost mana
  (Heal 100, Blessing 260).
- **Spellbook** : a grid of 3 columns × 5 rows of spell cards (icon,
  name, effect, mana, cast time). Empty slots are dark.
- **Wages:** each unit has a `Ежедневная выплата
  shows the total `выплата`.
- **Unit promotion** by XP through a branching class tree. XP thresholds seen: 400, 560, 580, 590, 800.
- **Paid healing** in barracks, with a price per unit.
- **Boats** on the river and bridges as choke points.
- **Neutral and friendly lord armies** roam, showing a leader and a formation.

---

## Prioritised list of what the remake lacks (compared with spec 2026-09-24)

1. **Daily tick at 12:00, not midnight.** The report window, income minus wages, and the autosave all
   happen at `12 час`. Change the midnight choice in the spec.
2. **Calendar format:** year/month/day/hour, **30-day months, 0-based days**, hour resolution. Add
   "time left on path" to the time panel.
3. **Battle UI parity:** enemy 2×6 grid on top and player 2×6 at the bottom, front rows facing in the
   middle; a left panel for the active unit's stats; action-preview tooltips with the damage number;
   green frame for the actor, blue frames for move cells, red tint for targets; empty-slot icons by
   row type.
4. **XP and levels plus a unit promotion tree** (floating XP on cards after a win, the upgrade tree on
   the army screen).
5. **Mana resource and spells:** a spellbook, a spell shop (the sanctuary), spells cast both on the map
   (costing game time) and in battle, buffs with durations, debuffs (−initiative, −actions), and area
   damage.
6. **Fort/castle capture:** a victory window with gold and mana, the fort's income added to the daily
   income, ownership shown in the tooltip and on the minimap colours.
7. **Fog of war** with a permanent explored memory, plus a **minimap** toggle.
8. **Hover tooltips** for armies, forts, villages and landmarks, including the army formation preview.
9. **Autosave before each battle and daily, plus a load screen.** ESC in battle gives only
   quit/menu/restart, with no retreat.
10. **Quests and journal:** dialog windows, "entry added" and "completed and removed" notices,
    money and item rewards; tavern rumours bought for 10 gold in the main hall.
11. **Barracks paid healing per unit;** a separate stat line for daily wages; a "dismiss unit" button.
12. **Stats model:** separate melee/ranged attack and defence; three magic-resistance schools
    (life, elements, death); regeneration %; initiative; actions; traits (flank bonus, second-row
    ranged defence, garrison bonus, trader discounts).
13. **Per-market stock and prices,** a separate sell shop, class-restricted items, % bonuses, vampirism.
14. Lower priority: boats and river travel, enemy lords who hunt the hero, friendly "meeting on the road"
    events, a companion leaving after a story event.

## UI layout notes for recreating the screens (placeholder art)

- The base resolution is 960×720. The bottom bar is the full width and about 80 px tall: two rows, 4 round
  buttons + a 260 px time panel + 4 round buttons, then the resource text strip.
- Modal windows are about 840×600, centred, with a 24 px title bar and a close ✕ at top-right.
  - Town: left tab column 245 px, content 580 px.
  - Battle and army: left portrait/stat panel 245 px, right a 6×2 (or 6×4 in battle) grid of 88×120
    cards with about 8 px gaps.
- Palette: dark green marble for frames, red-brown parchment for text, light parchment for the tab
  column, dark red for the battle background. Text: yellow for headings and prices, blue for buffed
  stats and system notices, red for debuffs, green for names.
- Dialog template: title, parchment text box, reward icon row (coins, mana crystal, house, knight),
  a blue notice line, and one or two buttons (`Ок` / `Да` `Нет`).
- The minimap is a 400×400 top-right overlay with icons per owner colour and a viewport rectangle.
- Tooltips are translucent panels with title → 2×6 mini grid → "Предводитель"/"Владелец" + name →
  description.

## Reference frames (in `ref/`)
| file | time | what |
|---|---|---|
| 01_town_main_hall.png | 06:09 | town window, main hall with quest/rumour list |
| 02_tavern_rumor.png | 06:25 | rumour bought for 10 gold |
| 03_barracks_hire_heal.png | 19:52 | barracks: 6 recruits, prices, per-unit heal buttons |
| 04_market.png | 16:30 | market list, description, buy/sell buttons |
| 05_sanctuary_spells.png | 07:11 | spell shop |
| 06_world_map_fog_bottom_bar.png | 37:02 | fog ellipse, explored area, bottom bar |
| 07_army_tooltip_minimap.png | 14:20 | army hover with a 2×6 preview, minimap |
| 08_village_tooltip.png | 10:37 | village tooltip "tribute already collected" |
| 09_battle_screen.png | 29:40 | battle layout, card stats, curse preview tooltip |
| 10_battle_end_xp_victory_sheet.png | 09:45–09:59 | XP badges on cards, fort-capture victory window |
| 11_hero_screen_upgrade_tree.png | 14:00 | army screen with the promotion tree |
| 12_hero_screen_inventory.png | 28:30 | hero inventory 5-wide grid |
| 13_heal_spell_on_army_screen.png | 10:50 | casting heal from the army screen |
| 14_load_autosaves.png | 29:20 | load screen, autosaves before battles and at noon |
| 15_spellbook_report_battle_sheet.png | 36:57–37:05 | spellbook, noon report, battle start |
