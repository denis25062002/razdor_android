//! Building services: the tabs of a town, castle, fort, village or church window and what
//! each tab does (mechanics.md 1.6, 3.1, 4, 5.3): hiring, paid healing and resurrection,
//! garrisons, the market, the sanctuary (spell shop) and village tribute.
//!
//! Only the building the hero stands in can be used. An ill-disposed building (attitude
//! below 0) still trades and heals, dearer at its market (see `Game::relation_markup`), but
//! does not hire or pay tribute.

use super::content::SpellDef;
use super::formation::Slot;
use super::game::{Event, Game, Price, PACK_SIZE, SPELL_BOOK_SIZE};
use super::units::Stats;
use super::world::{EventId, Location, LocationKind, Stationed};

/// A tab of a building window. "Exit" is the UI's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    /// Description, quests and rumours.
    MainHall,
    /// Hire, heal and (towns, churches) resurrect.
    Barracks,
    /// Leave or take troops (the player's castles and forts).
    Garrison,
    /// Buy and sell items.
    Market,
    /// Learn spells.
    Sanctuary,
    /// A village's tribute and its alternatives.
    Tribute,
    /// Rent a ship (`rules::ships`).
    Shipyard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError {
    /// This building does not offer it (or is hostile, or the hero is on the road).
    NotHere,
    /// Not enough gold, or mana for units paid in mana.
    CannotAfford,
    NotWounded,
    NotDead,
    /// The body is past `MaxTimeResurection`.
    TooLate,
    /// The dead cannot be left in a garrison.
    Dead,
    SquadFull,
    GarrisonFull,
    /// The hero cannot be left, dismissed or healed away from his army.
    Hero,
    AlreadyKnown,
    BookFull,
    PackFull,
    NoSuchUnit,
}

/// The tabs location `l` shows the player, in the original's order (mechanics.md 5.3):
/// - every building: the main hall;
/// - towns, castles, forts, churches: the barracks (hire and heal; towns and churches also
///   resurrect); villages and altars hire for the AI only;
/// - the player's castles and forts: the garrison;
/// - a building with goods: the market; with spells: the sanctuary;
/// - villages: the tribute; friendly shipyards: ships for rent.
///
/// Bridges, the demo's camps and a garrison still to be beaten have none.
pub fn tabs(l: &Location) -> Vec<Tab> {
    if l.kind.is_bridge() || l.kind == LocationKind::Camp || l.defended() {
        return Vec::new();
    }
    let mut tabs = vec![Tab::MainHall];
    if l.hires() {
        tabs.push(Tab::Barracks);
    }
    if l.takes_garrison() {
        tabs.push(Tab::Garrison);
    }
    if l.shop.is_some() {
        tabs.push(Tab::Market);
    }
    if !l.spells.is_empty() {
        tabs.push(Tab::Sanctuary);
    }
    if l.kind == LocationKind::Village {
        tabs.push(Tab::Tribute);
    }
    if l.kind == LocationKind::Shipyard && !l.hostile() {
        tabs.push(Tab::Shipyard);
    }
    tabs
}

/// The tab a building window opens on: a village's tribute, a shipyard's ships, else the
/// main hall.
pub fn first_tab(l: &Location) -> Option<Tab> {
    let t = tabs(l);
    t.iter().copied().find(|&t| matches!(t, Tab::Tribute | Tab::Shipyard)).or_else(|| t.first().copied())
}

impl Game {
    fn here(&self) -> Option<&Location> {
        self.location.map(|l| &self.world.locations[l])
    }

    /// Tabs of the building the hero stands in.
    pub fn tabs_here(&self) -> Vec<Tab> {
        self.here().map_or_else(Vec::new, tabs)
    }

    fn offers(&self, tab: Tab) -> bool {
        self.tabs_here().contains(&tab)
    }

    /// Quests and rumours listed in the main hall here (see [`World::local_events`]).
    ///
    /// [`World::local_events`]: super::world::World::local_events
    pub fn local_events(&self) -> Vec<EventId> {
        self.location.map_or_else(Vec::new, |l| self.world.local_events(l))
    }

    /// Paid healing is possible here: a friendly town, castle, fort or church.
    pub fn heals_here(&self) -> bool {
        self.offers(Tab::Barracks) && self.here().is_some_and(Location::heals)
    }

    /// Resurrection is possible here: a friendly town or church.
    pub fn resurrects_here(&self) -> bool {
        self.offers(Tab::Barracks) && self.here().is_some_and(Location::resurrects)
    }

    /// Price to heal squad member `i` fully: `Cost × HealingConst% × missing / max`, rounded
    /// up (mechanics.md 1.6), in mana for elementals. `None` if it is dead or unhurt.
    pub fn heal_price(&self, i: usize) -> Option<Price> {
        let u = self.squad.get(i)?;
        let max = u.max_hp(&self.content);
        if !u.alive() || u.hp >= max {
            return None;
        }
        let cost = self.content.unit(u.def).cost.max(0) as i64;
        let pct = self.content.options.healing_const.max(0) as i64;
        let missing = (max - u.hp) as i64;
        let denom = 100 * max as i64;
        let amount = (cost * pct * missing + denom - 1) / denom;
        Some(Price::for_unit(&self.content, u.def, amount as i32))
    }

    /// Heals squad member `i` to full HP for [`Game::heal_price`]; it takes `HealingTime`
    /// minutes of game time. Returns what happened meanwhile (a noon report …).
    pub fn heal(&mut self, i: usize) -> Result<Vec<Event>, ServiceError> {
        if !self.heals_here() {
            return Err(ServiceError::NotHere);
        }
        let u = self.squad.get(i).ok_or(ServiceError::NoSuchUnit)?;
        if !u.alive() {
            return Err(ServiceError::NotWounded);
        }
        let price = self.heal_price(i).ok_or(ServiceError::NotWounded)?;
        if !self.spend(price) {
            return Err(ServiceError::CannotAfford);
        }
        let c = self.content.clone();
        self.squad[i].heal_full(&c);
        Ok(self.spend_minutes(c.options.healing_time))
    }

    /// Minutes left to resurrect squad member `i`, if it is a corpse that can still be raised.
    pub fn resurrection_minutes_left(&self, i: usize) -> Option<u64> {
        let u = self.squad.get(i)?;
        let died = u.died_at.filter(|_| !u.alive())?;
        let end = died + self.content.options.max_time_resurection.max(0) as u64;
        end.checked_sub(self.clock.total_minutes() as u64)
    }

    /// Price to resurrect squad member `i`: `Cost × ResurectConst%` (mechanics.md 1.6).
    /// `None` if it is alive or past the window.
    pub fn resurrect_price(&self, i: usize) -> Option<Price> {
        self.resurrection_minutes_left(i)?;
        let u = &self.squad[i];
        let amount = self.content.unit(u.def).cost.max(0) * self.content.options.resurect_const.max(0) / 100;
        Some(Price::for_unit(&self.content, u.def, amount))
    }

    /// Raises the corpse of squad member `i` in a town or church, within `MaxTimeResurection`
    /// of its death. It comes back with full HP, and it takes `HealingTime` minutes, as a
    /// heal does *(guess)*.
    pub fn resurrect(&mut self, i: usize) -> Result<Vec<Event>, ServiceError> {
        if !self.resurrects_here() {
            return Err(ServiceError::NotHere);
        }
        let u = self.squad.get(i).ok_or(ServiceError::NoSuchUnit)?;
        if u.alive() {
            return Err(ServiceError::NotDead);
        }
        let price = self.resurrect_price(i).ok_or(ServiceError::TooLate)?;
        if !self.spend(price) {
            return Err(ServiceError::CannotAfford);
        }
        let c = self.content.clone();
        let u = &mut self.squad[i];
        u.died_at = None;
        u.heal_full(&c);
        Ok(self.spend_minutes(c.options.healing_time))
    }

    /// Game time spent on a service in a building.
    fn spend_minutes(&mut self, minutes: i32) -> Vec<Event> {
        let mut events = Vec::new();
        self.pass_time(minutes.max(0) as f32, &mut events);
        events
    }

    /// The player's troops in the garrison here.
    pub fn garrison_here(&self) -> &[Stationed] {
        match self.here() {
            Some(l) if l.takes_garrison() => &l.stationed,
            _ => &[],
        }
    }

    /// Leaves squad member `i` in the garrison of the castle or fort here. It keeps its
    /// items; its wage stops after its first day there, and it heals `GarrisonAutoHeal`% a
    /// day.
    pub fn leave_in_garrison(&mut self, i: usize) -> Result<(), ServiceError> {
        if !self.offers(Tab::Garrison) {
            return Err(ServiceError::NotHere);
        }
        if i == 0 {
            return Err(ServiceError::Hero);
        }
        let u = self.squad.get(i).ok_or(ServiceError::NoSuchUnit)?;
        if !u.alive() {
            return Err(ServiceError::Dead);
        }
        let l = self.location.ok_or(ServiceError::NotHere)?;
        let taken: Vec<Slot> = self.world.locations[l].stationed.iter().map(|s| s.unit.slot).collect();
        let f = self.content.formation;
        if taken.len() >= f.capacity() {
            return Err(ServiceError::GarrisonFull);
        }
        let row = Stats::of_level(&self.content, u.def, u.level).preferred_row();
        let slot = if taken.contains(&u.slot) { f.free_slot(&taken, row).ok_or(ServiceError::GarrisonFull)? } else { u.slot };
        let mut unit = self.squad.remove(i);
        unit.slot = slot;
        let since = self.clock.total_minutes() as u64;
        self.world.locations[l].stationed.push(Stationed { unit, since });
        Ok(())
    }

    /// Takes garrison unit `j` of the castle or fort here back into the squad.
    pub fn take_from_garrison(&mut self, j: usize) -> Result<(), ServiceError> {
        if !self.offers(Tab::Garrison) {
            return Err(ServiceError::NotHere);
        }
        let l = self.location.ok_or(ServiceError::NotHere)?;
        let s = self.world.locations[l].stationed.get(j).ok_or(ServiceError::NoSuchUnit)?;
        let taken: Vec<Slot> = self.squad.iter().map(|u| u.slot).collect();
        if self.squad.len() >= self.max_squad() {
            return Err(ServiceError::SquadFull);
        }
        let row = Stats::of_level(&self.content, s.unit.def, s.unit.level).preferred_row();
        let slot = if taken.contains(&s.unit.slot) {
            self.content.formation.free_slot(&taken, row).ok_or(ServiceError::SquadFull)?
        } else {
            s.unit.slot
        };
        let mut unit = self.world.locations[l].stationed.remove(j).unit;
        unit.slot = slot;
        self.squad.push(unit);
        Ok(())
    }

    /// Spells the sanctuary here teaches.
    pub fn spells_here(&self) -> Vec<&SpellDef> {
        if !self.offers(Tab::Sanctuary) {
            return Vec::new();
        }
        let l = self.here().expect("offers a sanctuary");
        l.spells.iter().filter_map(|&id| self.content.spells.iter().find(|s| s.id == id as u32)).collect()
    }

    pub fn knows_spell(&self, id: u32) -> bool {
        self.spells.iter().any(|&s| s as u32 == id)
    }

    /// Learns spell `id` in the sanctuary here for its `CostGold`; it goes into the hero's
    /// book (which holds [`SPELL_BOOK_SIZE`] spells), to be cast on the map (`rules::magic`).
    pub fn learn_spell(&mut self, id: u32) -> Result<(), ServiceError> {
        let spell = self.spells_here().into_iter().find(|s| s.id == id).ok_or(ServiceError::NotHere)?;
        let price = Price::gold(spell.cost_gold.max(0));
        if self.knows_spell(id) {
            return Err(ServiceError::AlreadyKnown);
        }
        if self.spells.len() >= SPELL_BOOK_SIZE {
            return Err(ServiceError::BookFull);
        }
        if !self.spend(price) {
            return Err(ServiceError::CannotAfford);
        }
        self.spells.push(id as u8);
        Ok(())
    }

    /// Sends squad member `i` away (the army screen's "dismiss"), or buries a corpse. Its
    /// items go to the pack, so the pack must have room for them.
    pub fn dismiss(&mut self, i: usize) -> Result<(), ServiceError> {
        if i == 0 {
            return Err(ServiceError::Hero);
        }
        let u = self.squad.get(i).ok_or(ServiceError::NoSuchUnit)?;
        let items: Vec<_> = u.items.iter().flatten().copied().collect();
        if self.pack.len() + items.len() > PACK_SIZE {
            return Err(ServiceError::PackFull);
        }
        self.squad.remove(i);
        self.pack.extend(items);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::dt::dtm::{BuildingType, RecruitSlot, Scenario};
    use crate::rules::battle::{Battle, Team};
    use crate::rules::content::testkit as ck;
    use crate::rules::content::{ArtefactType, Bonus, Content, HeroClass, ItemId, MagicDirection, MagicSchool, Nature, UnitDef, UnitId};
    use crate::rules::game::{BattleResult, Currency, DayReport, Foe, Game};
    use crate::rules::world::testkit::{army, building, hero, scenario, troop};

    /// Units: 1–3 heroes; 4 militia (cost 50); 5 archer (cost 90); 6 priest (cost 60,
    /// Surrender 20); 7 merchant (cost 100, Merchant); 8 golem (cost 80, Elemental); 9 bandit
    /// (cost 55, Rogue). Items: 20 sword (100), 21 ring (40), 22 potion (60), 23 ring (300),
    /// 24 amulet (1000). Spells 1 (200 gold) and 2 (500 gold).
    fn content() -> Content {
        let mut units = vec![
            ck::warrior(1, 20, 5),
            ck::mage(2, 10, MagicSchool::Elemental, MagicDirection::ToEnemy),
            ck::shooter(3, 15),
            UnitDef { cost: 50, hits: 40, ..ck::warrior(4, 10, 2) },
            UnitDef { cost: 90, ..ck::shooter(5, 8) },
            UnitDef { cost: 60, surrender: 20, ..ck::mage(6, 8, MagicSchool::Life, MagicDirection::ToAlly) },
            UnitDef { cost: 100, bonus: Some(Bonus::Merchant), ..ck::warrior(7, 5, 1) },
            UnitDef { cost: 80, nature: Nature::Elemental, ..ck::warrior(8, 12, 4) },
            UnitDef { cost: 55, nature: Nature::Rogue, ..ck::warrior(9, 9, 1) },
        ];
        for u in &mut units[..3] {
            u.cost = 0;
        }
        let item = |id, kind, cost| crate::rules::content::ArtefactDef { cost, ..ck::item(id, kind) };
        let items = vec![
            item(20, ArtefactType::BlowWeapon, 100),
            item(21, ArtefactType::Ring, 40),
            item(22, ArtefactType::Potion, 60),
            item(23, ArtefactType::Ring, 300),
            item(24, ArtefactType::Amulet, 1000),
        ];
        let spells = vec![ck::spell(1, 200), ck::spell(2, 500)];
        Content::new(units, items, spells, Default::default(), crate::rules::formation::Formation::WIDE)
    }

    /// A 24×8 grass map; the knight starts at (2, 2) with 1000 gold and two militia.
    fn map() -> Scenario {
        let mut s = scenario(24, 8);
        s.header.heroes[0] = hero(2, 2, 1000, &[troop(4, 0, 2)]);
        s
    }

    fn start(s: &Scenario) -> Game {
        Game::from_scenario(Arc::new(content()), s, HeroClass::Knight, 3)
    }

    fn town(kind: BuildingType, x: u16, y: u16, attitude: i8) -> crate::dt::dtm::Building {
        let mut b = building(kind, x, y, (1, 1));
        b.relations = [attitude, 0, 0, 0];
        b.faction = if attitude < 0 { 4 } else { 3 };
        b
    }

    /// A game standing in building 0 of `s`.
    fn inside(s: &Scenario) -> Game {
        let mut g = start(s);
        g.location = Some(0);
        g
    }

    #[test]
    fn tabs_follow_the_building_type() {
        let mut s = map();
        let mut t = town(BuildingType::Town, 5, 5, 1);
        t.barracks[0] = RecruitSlot { unit: 4, start_count: 2, max_count: 4 };
        t.random_artifacts_for_sale = 3;
        t.spells_for_sale[0] = 1;
        let mut castle = town(BuildingType::Castle, 8, 5, 3);
        castle.faction = 1;
        let fort = town(BuildingType::Fort, 11, 5, 1);
        let village = town(BuildingType::Village, 14, 5, 1);
        let mut church = town(BuildingType::Church, 17, 5, 1);
        church.spells_for_sale[0] = 2;
        let tavern = town(BuildingType::Tavern, 20, 5, 1);
        let mut market = town(BuildingType::Market, 22, 5, 1);
        market.random_artifacts_for_sale = 5;
        let mut hostile = town(BuildingType::Town, 5, 7, -2);
        hostile.random_artifacts_for_sale = 3;
        hostile.garrison[0] = troop(4, 0, 1);
        let bridge = town(BuildingType::StoneBridge, 3, 7, 1);
        s.buildings = vec![t, castle, fort, village, church, tavern, market, hostile, bridge];
        let g = start(&s);
        let tabs: Vec<Vec<Tab>> = g.world.locations.iter().map(tabs).collect();
        use Tab::*;
        assert_eq!(tabs[0], [MainHall, Barracks, Market, Sanctuary]);
        assert_eq!(tabs[1], [MainHall, Barracks, Garrison], "the player's castle");
        assert_eq!(tabs[2], [MainHall, Barracks], "a friendly fort: hire and heal only");
        assert_eq!(tabs[3], [MainHall, Tribute]);
        assert_eq!(tabs[4], [MainHall, Barracks, Sanctuary]);
        assert_eq!(tabs[5], [MainHall]);
        assert_eq!(tabs[6], [MainHall, Market]);
        assert_eq!(tabs[7], [MainHall, Barracks, Market], "an ill-disposed town trades (its garrison is AI-only)");
        assert!(tabs[8].is_empty());
        let l = &g.world.locations;
        assert!(l[0].heals() && l[0].resurrects() && l[1].heals() && !l[1].resurrects() && l[4].resurrects());
    }

    #[test]
    fn heal_costs_a_share_of_the_unit_cost_and_an_hour() {
        let mut s = map();
        s.buildings = vec![town(BuildingType::Town, 2, 2, 1)];
        let mut g = inside(&s);
        assert_eq!(g.heal_price(1), None, "unhurt");
        g.squad[1].hp = 10; // 30 of 40 missing: 50 × 50% × 30/40 = 18.75 → 19
        assert_eq!(g.heal_price(1), Some(Price::gold(19)));
        g.squad[2].hp = 39; // 50 × 50% × 1/40 = 0.625 → 1
        assert_eq!(g.heal_price(2), Some(Price::gold(1)));
        let (gold, t) = (g.gold, g.clock.total_minutes());
        g.heal(1).unwrap();
        assert_eq!((g.squad[1].hp, g.gold), (40, gold - 19));
        assert_eq!(g.clock.total_minutes(), t + 60.0, "HealingTime");
        assert_eq!(g.heal(1), Err(ServiceError::NotWounded));
        g.gold = 0;
        assert_eq!(g.heal(2), Err(ServiceError::CannotAfford));
        g.location = None;
        assert_eq!(g.heal(2), Err(ServiceError::NotHere));
    }

    #[test]
    fn healing_across_noon_brings_the_report() {
        let mut s = map();
        s.buildings = vec![town(BuildingType::Church, 2, 2, 1)];
        let mut g = inside(&s);
        g.pass_time(2.5 * 60.0, &mut Vec::new()); // 09:00 -> 11:30
        g.squad[1].hp = 1;
        let events = g.heal(1).unwrap();
        assert!(matches!(events.as_slice(), [Event::NewDay(_)]), "{events:?}");
    }

    #[test]
    fn no_healing_in_taverns_or_villages() {
        let mut s = map();
        s.buildings = vec![town(BuildingType::Tavern, 6, 2, 1), town(BuildingType::Village, 9, 2, 1), town(BuildingType::Church, 12, 2, -2)];
        let mut g = inside(&s);
        g.squad[1].hp = 1;
        for l in 0..2 {
            g.location = Some(l);
            assert_eq!(g.heal(1), Err(ServiceError::NotHere), "{l}");
        }
        g.location = Some(2);
        assert!(g.heal(1).is_ok(), "an ill-disposed church still heals");
    }

    fn bandit_army() -> crate::rules::world::Army {
        let mut s = map();
        s.armies = vec![army(1, 20, 6, -2, &[troop(9, 0, 1)])];
        start(&s).world.armies.remove(0)
    }

    /// Kills squad member `i` in a won battle against a one-bandit army.
    fn lose_unit_in_battle(g: &mut Game, i: usize) {
        g.world.armies.push(bandit_army());
        g.foe = Some(Foe::Army(g.world.armies.len() - 1));
        let mut b = g.start_battle();
        b.begin();
        for f in b.fighters.iter_mut() {
            if f.team == Team::Enemy || f.squad_index == Some(i) {
                f.hp = 0;
            }
        }
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { lost: 1, .. }));
    }

    #[test]
    fn the_dead_stay_as_corpses_and_can_be_raised_in_a_town_within_a_week() {
        let mut s = map();
        s.buildings = vec![town(BuildingType::Town, 2, 2, 1), town(BuildingType::Castle, 6, 2, 2)];
        let mut g = inside(&s);
        g.squad[1].items[0] = Some(ItemId(20));
        lose_unit_in_battle(&mut g, 1);
        assert_eq!(g.squad.len(), 3, "the corpse stays in the army");
        let corpse = &g.squad[1];
        assert!(!corpse.alive() && corpse.died_at.is_some());
        assert!(corpse.items.iter().all(Option::is_none) && g.pack.contains(&ItemId(20)), "the dead hold no items");
        assert_eq!(g.heal_price(1), None);
        // Resurrection: Cost × 300%.
        assert_eq!(g.resurrect_price(1), Some(Price::gold(150)));
        assert_eq!(g.resurrection_minutes_left(1), Some(10_080));
        g.location = Some(1);
        assert_eq!(g.resurrect(1), Err(ServiceError::NotHere), "castles heal but do not resurrect");
        g.location = Some(0);
        let gold = g.gold;
        g.resurrect(1).unwrap();
        assert_eq!((g.squad[1].hp, g.squad[1].died_at, g.gold), (40, None, gold - 150));
        assert_eq!(g.resurrect(1), Err(ServiceError::NotDead));
        // Corpses do not fight and are not paid.
        lose_unit_in_battle(&mut g, 2);
        assert_eq!(g.wage(2), 0);
        let b = g.start_battle();
        assert!(b.fighters.iter().all(|f| f.squad_index != Some(2)));
    }

    #[test]
    fn corpses_past_the_window_are_buried() {
        let mut s = map();
        s.buildings = vec![town(BuildingType::Church, 2, 2, 1)];
        let mut g = inside(&s);
        lose_unit_in_battle(&mut g, 1);
        g.pass_time(7.0 * 24.0 * 60.0, &mut Vec::new());
        assert_eq!(g.resurrection_minutes_left(1), Some(0), "exactly the last minute");
        assert!(g.resurrect_price(1).is_some());
        g.pass_time(1.0, &mut Vec::new());
        assert_eq!(g.squad.len(), 2, "buried");
    }

    #[test]
    fn barracks_stock_goes_down_and_regrows() {
        let mut s = map();
        let mut t = town(BuildingType::Town, 2, 2, 1);
        t.barracks[0] = RecruitSlot { unit: 4, start_count: 1, max_count: 5 };
        t.barracks[1] = RecruitSlot { unit: 5, start_count: 0, max_count: 1 };
        s.buildings = vec![t];
        let mut g = inside(&s);
        assert_eq!(g.recruits_here(), vec![UnitId(4)], "no archers yet");
        g.hire(UnitId(4)).unwrap();
        assert_eq!(g.gold, 950);
        assert_eq!(g.hire(UnitId(4)), Err(crate::rules::game::HireError::NotOffered), "sold out");
        // MaxDayCountForNewUnit = 10: 5 militia regrow in 10 days, one every 2 days.
        let day = 24.0 * 60.0;
        g.pass_time(15.0 * 60.0, &mut Vec::new()); // the first midnight
        assert_eq!(g.world.locations[0].recruits[0].stock, Some(0));
        g.pass_time(day, &mut Vec::new());
        assert_eq!(g.world.locations[0].recruits[0].stock, Some(1));
        g.pass_time(7.0 * day, &mut Vec::new());
        assert_eq!(g.world.locations[0].recruits[0].stock, Some(4));
        g.pass_time(10.0 * day, &mut Vec::new());
        assert_eq!(g.world.locations[0].recruits[0].stock, Some(5), "capped at the maximum");
        assert_eq!(g.world.locations[0].recruits[1].stock, Some(1), "one archer in 10 days");
    }

    #[test]
    fn elementals_are_hired_healed_and_paid_in_mana() {
        let mut s = map();
        let mut t = town(BuildingType::Town, 2, 2, 1);
        t.barracks[0] = RecruitSlot { unit: 8, start_count: 3, max_count: 3 };
        s.buildings = vec![t];
        let mut g = inside(&s);
        assert_eq!(g.hire_price(UnitId(8)), Price { amount: 80, currency: Currency::Mana });
        assert_eq!(g.hire(UnitId(8)), Err(crate::rules::game::HireError::NotEnoughGold), "no mana");
        g.mana = 100;
        g.hire(UnitId(8)).unwrap();
        assert_eq!((g.mana, g.gold), (20, 1000));
        g.squad[3].hp = 25; // of 50: 80 × 50% × 1/2 = 20 mana
        assert_eq!(g.heal_price(3), Some(Price { amount: 20, currency: Currency::Mana }));
        // Wage 80/2 × ½ = 20 mana a day.
        assert_eq!((g.daily_wages(), g.daily_mana_wages()), (12, 20));
    }

    #[test]
    fn noon_pays_both_wage_kinds_and_reports_the_balance() {
        let mut s = map();
        s.header.heroes[0] = hero(2, 2, 100, &[troop(4, 0, 1), troop(9, 0, 1)]);
        let mut fort = town(BuildingType::Fort, 8, 2, 3);
        fort.faction = 1;
        fort.gold_per_day = 40;
        fort.mana_per_day = 7;
        let mut village = town(BuildingType::Village, 12, 2, 1);
        village.gold_per_day = 500; // tribute, not income
        s.buildings = vec![fort, village];
        let mut g = start(&s);
        // Militia kind 1: 50/2 × ¼ = 6.25 → 6. Bandit (rogue) kind 2: 55/2 = 27.
        assert_eq!((g.wage(1), g.wage(2)), (6, 27));
        assert_eq!((g.daily_income(), g.daily_mana(), g.daily_wages()), (40, 7, 33));
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events); // 09:00 -> 12:00
        let day = g.clock.day_index();
        let want = DayReport {
            day,
            income: 40,
            mana: 7,
            wages: 33,
            mana_wages: 0,
            unpaid: 0,
            deserted: vec![],
            gold: 100 + 40 - 33,
            mana_total: 7,
        };
        assert_eq!(events, vec![Event::NewDay(want)]);
    }

    #[test]
    fn unpaid_units_sit_out_and_desert_after_a_week() {
        let mut s = map();
        s.header.heroes[0] = hero(2, 2, 0, &[troop(4, 0, 2)]);
        s.buildings = vec![town(BuildingType::Village, 12, 2, 1)];
        let mut g = start(&s);
        g.squad[2].items[0] = Some(ItemId(21));
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events);
        let Event::NewDay(r) = &events[0] else { panic!() };
        assert_eq!((r.wages, r.unpaid), (0, 2));
        assert!(g.squad[1].unpaid && g.squad[2].unpaid);
        let b = g.start_battle();
        assert_eq!(b.fighters.iter().filter(|f| f.team == Team::Player).count(), 1, "only the hero fights");
        // Paid again as soon as there is gold.
        g.gold = 6;
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        assert!(!g.squad[1].unpaid && g.squad[2].unpaid);
        // MaxTimeNotUpkeep = 7 days unpaid: the second militia leaves; its ring stays.
        g.gold = 0;
        events.clear();
        g.pass_time(5.0 * 24.0 * 60.0, &mut events);
        let deserted: Vec<UnitId> = events.iter().flat_map(|e| match e {
            Event::NewDay(r) => r.deserted.clone(),
            _ => vec![],
        }).collect();
        assert_eq!(deserted, vec![UnitId(4)]);
        assert_eq!(g.squad.len(), 2);
        assert!(g.pack.contains(&ItemId(21)));
    }

    #[test]
    fn the_village_innkeeper_pays_off_the_unpaid_instead_of_tribute() {
        let mut s = map();
        let mut v = town(BuildingType::Village, 2, 2, 1);
        v.gold_per_day = 30;
        v.gold_max = 90;
        s.buildings = vec![v];
        let mut g = inside(&s);
        g.squad[1].unpaid = true;
        g.squad[1].unpaid_days = 3;
        assert_eq!(g.innkeeper_pay(), Some(1));
        assert!(!g.squad[1].unpaid && g.squad[1].unpaid_days == 0);
        assert_eq!(g.tribute_available(), None, "used up for today");
        assert_eq!(g.innkeeper_pay(), None);
    }

    #[test]
    fn village_tribute_accumulates_to_its_cap_and_is_collected_on_a_visit() {
        let mut s = map();
        let mut v = town(BuildingType::Village, 2, 2, 1);
        v.gold_per_day = 30;
        v.gold_max = 70;
        v.mana_per_day = 10;
        v.mana_max = 25;
        s.buildings = vec![v];
        let mut g = inside(&s);
        assert_eq!(g.tribute_available(), Some(30), "one day's worth at the start");
        g.pass_time(5.0 * 24.0 * 60.0, &mut Vec::new());
        let v = &g.world.locations[0];
        assert_eq!((v.tribute_gold, v.tribute_mana), (70, 25), "capped");
        let (gold, mana) = (g.gold, g.mana);
        assert!(g.collect_tribute().is_some());
        assert_eq!((g.gold, g.mana), (gold + 70, mana + 25));
        assert_eq!(g.tribute_available(), None);
    }

    #[test]
    fn garrisons_take_troops_whose_wage_stops_after_a_day_and_who_heal() {
        let mut s = map();
        let mut castle = town(BuildingType::Castle, 2, 2, 3);
        castle.faction = 1;
        s.buildings = vec![castle, town(BuildingType::Castle, 8, 2, 1)];
        let mut g = inside(&s);
        assert_eq!(g.leave_in_garrison(0), Err(ServiceError::Hero));
        g.squad[2].hp = 20; // of 40
        g.leave_in_garrison(2).unwrap();
        assert_eq!((g.squad.len(), g.garrison_here().len()), (2, 1));
        assert_eq!(g.daily_wages(), 12, "still paid on its first day");
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events); // noon: 3 h after leaving
        assert!(matches!(&events[..], [Event::NewDay(DayReport { wages: 12, .. })]), "{events:?}");
        assert_eq!(g.garrison_here()[0].unit.hp, 24, "GarrisonAutoHeal 10%");
        assert_eq!(g.daily_wages(), 6, "from its second day, free");
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        assert!(matches!(&events[..], [Event::NewDay(DayReport { wages: 6, .. })]));
        g.take_from_garrison(0).unwrap();
        assert_eq!((g.squad.len(), g.squad[2].hp), (3, 28));
        assert_eq!(g.take_from_garrison(0), Err(ServiceError::NoSuchUnit));
        g.location = Some(1);
        assert_eq!(g.leave_in_garrison(1), Err(ServiceError::NotHere), "not the player's castle");
    }

    #[test]
    fn a_scenario_garrison_of_an_own_castle_is_the_players_and_free() {
        let mut s = map();
        let mut castle = town(BuildingType::Castle, 2, 2, 3);
        castle.faction = 1;
        castle.garrison[0] = troop(4, 0, 3);
        s.buildings = vec![castle];
        let g = inside(&s);
        assert_eq!(g.garrison_here().len(), 3);
        assert!(g.world.locations[0].garrison.is_empty());
        assert_eq!(g.daily_wages(), 12, "only the two in the squad");
    }

    fn shop_town(attitude: i8) -> Scenario {
        let mut s = map();
        let mut t = town(BuildingType::Market, 2, 2, attitude);
        t.artifact_slots[0] = 24;
        t.random_artifacts_for_sale = 2;
        t.price_min = 50;
        t.price_max = 400;
        s.buildings = vec![t];
        s
    }

    #[test]
    fn markets_stock_fixed_goods_and_random_items_in_their_price_range() {
        let g = inside(&shop_town(2));
        let stock = g.market_here().unwrap().to_vec();
        assert_eq!(stock.len(), 3);
        assert_eq!(stock[0], ItemId(24), "the fixed goods, whatever their price");
        let mut random = stock[1..].to_vec();
        random.sort();
        // Market items between 50 and 400: the sword (100), the potion (60), the ring (300).
        assert!(random.iter().all(|i| [20, 22, 23].contains(&i.0)), "{random:?}");
        assert_ne!(random[0], random[1], "different items");
    }

    #[test]
    fn fixed_goods_do_not_come_back_but_the_random_ones_restock_weekly() {
        let mut g = inside(&shop_town(2));
        g.gold = 100_000;
        g.buy(0).unwrap();
        assert_eq!(g.pack, vec![ItemId(24)]);
        g.buy(0).unwrap();
        assert_eq!(g.market_here().unwrap().len(), 1);
        // The 8th noon from the start: a week after the first.
        g.pass_time(7.0 * 24.0 * 60.0 + 3.0 * 60.0, &mut Vec::new());
        let stock = g.market_here().unwrap().to_vec();
        assert_eq!(stock.len(), 2, "two random items again");
        assert!(!stock.contains(&ItemId(24)), "the fixed amulet is sold for good");
    }

    #[test]
    fn prices_follow_relation_merchant_and_sale_percent() {
        for (attitude, want) in [(3, 1000), (1, 1000), (0, 1150), (-2, 1450)] {
            let g = inside(&shop_town(attitude));
            assert_eq!(g.buy_price(ItemId(24)), want, "attitude {attitude}");
        }
        let mut s = shop_town(-2);
        s.header.heroes[0] = hero(2, 2, 5000, &[troop(7, 0, 1)]);
        let mut g = inside(&s);
        // The footage: a trader hero pays 122 for a 120 dagger at an attitude −2 market.
        assert_eq!(g.buy_price(ItemId(24)), 1450 * 70 / 100, "Merchant: −30%");
        g.pack = vec![ItemId(23)];
        assert_eq!(g.sell_price(ItemId(23)), 300 * 25 / 100 * 150 / 100, "ItemSaleCost 25%, Merchant +50%");
        assert_eq!(g.sell(0), Ok(112));
        let g = inside(&shop_town(1));
        assert_eq!(g.sell_price(ItemId(23)), 75);
    }

    #[test]
    fn ill_disposed_buildings_trade_but_do_not_hire_or_pay_tribute() {
        let g = inside(&shop_town(-1));
        assert_eq!(g.market_here().map(<[ItemId]>::len), Some(3));
        let mut s = map();
        let mut t = town(BuildingType::Town, 2, 2, -1);
        t.barracks[0] = RecruitSlot { unit: 4, start_count: 3, max_count: 3 };
        let mut v = town(BuildingType::Village, 6, 2, -1);
        v.gold_per_day = 20;
        s.buildings = vec![t, v];
        let mut g = inside(&s);
        assert!(g.recruits_here().is_empty());
        g.location = Some(1);
        assert_eq!(g.tribute_available(), None);
    }

    #[test]
    fn walking_into_an_empty_hostile_fort_takes_it() {
        let mut s = map();
        let mut fort = town(BuildingType::Fort, 8, 2, -2);
        fort.gold_per_day = 25;
        s.buildings = vec![fort];
        let mut g = start(&s);
        assert!(g.set_destination(g.world.locations[0].tile));
        let mut events = Vec::new();
        for _ in 0..1000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        assert!(events.contains(&Event::Captured(0)), "{events:?}");
        assert!(g.world.locations[0].owned() && g.foe.is_none());
        assert_eq!(g.daily_income(), 25);
    }

    #[test]
    fn sanctuaries_teach_spells_for_gold_into_the_book() {
        let mut s = map();
        let mut church = town(BuildingType::Church, 2, 2, 1);
        church.spells_for_sale = [1, 2, 0, 0, 0, 0];
        s.buildings = vec![church];
        let mut g = inside(&s);
        assert_eq!(g.spells_here().iter().map(|sp| (sp.id, sp.cost_gold)).collect::<Vec<_>>(), [(1, 200), (2, 500)]);
        g.learn_spell(1).unwrap();
        assert_eq!((g.gold, g.spells.clone()), (800, vec![1]));
        assert_eq!(g.learn_spell(1), Err(ServiceError::AlreadyKnown));
        g.gold = 499;
        assert_eq!(g.learn_spell(2), Err(ServiceError::CannotAfford));
        g.gold = 500;
        g.spells = (3..18).collect();
        assert_eq!(g.learn_spell(2), Err(ServiceError::BookFull), "15 cells in the book");
        assert_eq!(g.learn_spell(7), Err(ServiceError::NotHere));
    }

    #[test]
    fn dismissing_a_unit_returns_its_items() {
        let mut g = start(&map());
        g.squad[1].items[0] = Some(ItemId(20));
        assert_eq!(g.dismiss(0), Err(ServiceError::Hero));
        g.dismiss(1).unwrap();
        assert_eq!((g.squad.len(), g.pack.clone()), (2, vec![ItemId(20)]));
        g.squad[1].items[0] = Some(ItemId(21));
        g.pack = vec![ItemId(22); PACK_SIZE];
        assert_eq!(g.dismiss(1), Err(ServiceError::PackFull));
    }

    #[test]
    fn victory_loot_is_half_the_gold_with_a_floor_or_all_of_it() {
        let g = start(&map());
        // VictoryGoldDiv 2, MinVictoryGold 25.
        assert_eq!([0, 10, 25, 40, 50, 120].map(|x| g.victory_gold(x)), [0, 10, 25, 25, 25, 60]);
    }

    #[test]
    fn capturing_a_fort_pays_a_day_of_income_and_surrender_mana_and_raises_income() {
        let mut s = map();
        let mut fort = town(BuildingType::Fort, 8, 2, -1);
        fort.gold_per_day = 30;
        fort.garrison[0] = troop(9, 0, 2);
        fort.garrison[1] = troop(6, 0, 1);
        s.buildings = vec![fort];
        let mut g = start(&s);
        g.location = Some(0);
        g.foe = Some(Foe::Garrison(0));
        let mut b: Battle = g.start_battle();
        b.begin();
        // The bandits fall; the remaining priest (Surrender 20) gives up after the next action.
        b.fighters.iter_mut().filter(|f| f.team == Team::Enemy && f.surrender == 0).for_each(|f| f.hp = 0);
        b.pass();
        let (gold, mana) = (g.gold, g.mana);
        let r = g.resolve_battle(&b);
        assert!(matches!(r, BattleResult::Victory { reward: 30, mana: 20, captured: Some(0), .. }), "{r:?}");
        assert_eq!((g.gold, g.mana), (gold + 30, mana + 20));
        assert_eq!(g.daily_income(), 30, "its income counts at once");
        assert_eq!(g.tabs_here(), vec![Tab::MainHall, Tab::Barracks, Tab::Garrison]);
    }
}

#[cfg(test)]
mod real_maps {
    //! Checks against the player's install; skipped without `RAZDOR_DT_DIR`. Only numbers
    //! and ids are compared, all read from the player's files.
    use std::sync::Arc;

    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::{Content, HeroClass, UnitId};
    use crate::rules::world::LocationKind;

    #[test]
    fn rk1_home_castle_and_the_first_friendly_church_offer_their_file_stock() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        let s = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap().load().unwrap();
        let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight, 11);
        g.world.armies.clear();

        let walk_into = |g: &mut Game, l: usize| {
            if g.location != Some(l) {
                assert!(g.set_destination(g.world.locations[l].tile));
                for _ in 0..50_000 {
                    if !g.moving() {
                        break;
                    }
                    g.tick(0.05);
                }
            }
            assert_eq!(g.location, Some(l), "arrived");
        };

        // His own castle, next to his start: barracks with the file's stock, a garrison.
        let home = g.world.locations.iter().position(|l| l.owned() && l.kind == LocationKind::Castle).expect("a home castle");
        walk_into(&mut g, home);
        let b = &s.buildings[home];
        assert!(g.world.locations[home].owned());
        assert_eq!(g.tabs_here()[..3], [Tab::MainHall, Tab::Barracks, Tab::Garrison]);
        let file: Vec<(u32, i32, i32)> = b
            .barracks
            .iter()
            .filter(|r| r.unit != 0)
            .map(|r| (r.unit as u32, r.start_count as i32, r.max_count.max(r.start_count) as i32))
            .collect();
        let ours: Vec<(u32, i32, i32)> = g.world.locations[home].recruits.iter().map(|r| (r.unit.0, r.stock.unwrap(), r.max)).collect();
        assert_eq!(ours, file);

        // Walk into the nearest friendly building that hires.
        let (l, _) = g
            .world
            .nearest_location(g.tile(), |l| l.hires() && !l.hostile() && !l.owned() && l.kind != LocationKind::Castle)
            .expect("a friendly town or church");
        walk_into(&mut g, l);
        let b = &s.buildings[l];
        let tabs = g.tabs_here();
        assert!(tabs.contains(&Tab::Barracks));
        let offered: Vec<UnitId> = b.barracks.iter().filter(|r| r.unit != 0 && r.start_count > 0).map(|r| UnitId(r.unit as u32)).collect();
        assert_eq!(g.recruits_here(), offered);
        if b.random_artifacts_for_sale > 0 || b.artifacts().next().is_some() {
            assert!(tabs.contains(&Tab::Market));
            let stock = g.market_here().unwrap();
            let fixed: Vec<u16> = b.artifacts().collect();
            assert_eq!(stock.len(), fixed.len() + b.random_artifacts_for_sale as usize);
            for item in &stock[fixed.len()..] {
                let cost = c.item(*item).cost;
                assert!((b.price_min as i32..=b.price_max as i32).contains(&cost), "{cost} outside the range");
            }
        }
        let spells: Vec<u32> = b.spells_for_sale.iter().filter(|&&x| x != 0).map(|&x| x as u32).collect();
        assert_eq!(g.spells_here().iter().map(|sp| sp.id).collect::<Vec<_>>(), spells);
        assert_eq!(tabs.contains(&Tab::Sanctuary), !spells.is_empty());
    }
}
