//! The AI on small synthetic scenarios.
use std::sync::Arc;

use super::*;
use crate::dt::data::AiTargets;
use crate::rules::rng::Rng;
use crate::dt::dtm::{Army as DtArmy, BuildingType, Scenario, Troop as DtTroop};
use crate::rules::content::testkit as ck;
use crate::rules::content::{ArtefactType, HeroClass, ItemId, UnitDef};
use crate::rules::world::testkit::{self as tk, building, hero, scenario, troop};

/// Units 1–3 heroes, 4 a weak warrior, 5 a shooter, 6 a strong warrior, 7 a rogue warrior;
/// item 7 a ring, 9 a potion.
fn content() -> Content {
    let units = vec![
        ck::warrior(1, 20, 5),
        ck::warrior(2, 20, 5),
        ck::warrior(3, 20, 5),
        ck::warrior(4, 10, 2),
        ck::shooter(5, 8),
        UnitDef { hits: 120, cost: 200, ..ck::warrior(6, 60, 20) },
        UnitDef { nature: Nature::Rogue, cost: 60, ..ck::warrior(7, 12, 2) },
    ];
    ck::content(units, vec![ck::item(7, ArtefactType::Ring), ck::item(9, ArtefactType::Potion)])
}

/// A 60 × 20 grass map; the hero stands far away in the corner (0, 0).
fn map() -> Scenario {
    let mut s = scenario(60, 20);
    s.header.heroes[0] = hero(0, 0, 100, &[troop(4, 0, 1)]);
    s
}

/// An army of `faction` with `relations` towards the four factions, style byte `style`.
fn army(id: u8, at: (u16, u16), faction: u8, relations: [i8; 4], style: u8, troops: &[DtTroop]) -> DtArmy {
    let mut a = tk::army(id, at.0, at.1, relations[0], troops);
    a.faction = faction;
    a.relations = relations;
    a.behaviour = style;
    a.model = 4 + style;
    a
}

const ALLY: [i8; 4] = [1, 3, 1, -2];
const ENEMY: [i8; 4] = [0, -2, 1, 3];

fn start(s: &Scenario) -> Game {
    start_with(s, content())
}

fn start_with(s: &Scenario, c: Content) -> Game {
    let mut g = Game::from_scenario(Arc::new(c), s, HeroClass::Knight);
    g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
    g
}

fn goal(g: &mut Game, i: usize) -> Goal {
    g.ai_choice(i).0
}

fn by_id(g: &Game, id: u8) -> Option<&Army> {
    g.world.armies.iter().find(|a| a.id == id)
}

#[test]
fn style_is_byte_59_else_the_map_model() {
    assert_eq!(Style::of(0, 7), Style::Feudal);
    assert_eq!(Style::of(1, 4), Style::Rogue);
    assert_eq!(Style::of(2, 7), Style::Peasant);
    assert_eq!(Style::of(9, 5), Style::Rogue);
    assert_eq!(Style::of(9, 6), Style::Peasant);
    assert_eq!(Style::of(9, 4), Style::Feudal);
}

#[test]
fn priorities_by_target_model_and_missing_keys() {
    let mut o = GlobalOptions::default();
    assert_eq!(Priorities::of(&o, 1), DEMO_PRIORITIES, "no _Global.ini: our own values");
    o.ai_targets = AiTargets {
        min_attack_army: Some([1, 2, 3, 4, 5]),
        min_random: Some([10, 20, 30, 40, 50]),
        max_healing: Some([100, 200, 300, 400, 500]),
        ..AiTargets::default()
    };
    let p = Priorities::of(&o, model::PASSIVE);
    assert_eq!((p.attack_army, p.random, p.heal), (3, 30, (0, 300)), "a missing Min reads 0");
    assert_eq!(lerp((0, 300), 0), 300);
    assert_eq!(lerp((0, 300), 1000), 0);
    assert_eq!(lerp((100, 300), 500), 200);
}

#[test]
fn target_range_is_ai_distance_by_style() {
    let o = GlobalOptions::default();
    assert_eq!(target_range(&o, Style::Feudal), 100);
    assert_eq!(target_range(&o, Style::Rogue), 50);
    assert_eq!(target_range(&o, Style::Peasant), 25);
}

#[test]
fn armies_farther_than_their_range_are_no_targets() {
    let mut s = scenario(160, 20);
    s.header.heroes[0] = hero(0, 0, 100, &[troop(4, 0, 1)]);
    // A rogue (range 50) and a lord (range 100), both 60 cells from a weak enemy.
    s.armies = vec![army(1, (10, 10), 2, ALLY, 1, &[troop(6, 0, 3)]), army(2, (70, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]), army(3, (130, 10), 2, ALLY, 0, &[troop(6, 0, 3)])];
    let mut g = start(&s);
    assert_ne!(goal(&mut g, 0), Goal::AttackArmy(2), "60 cells: beyond a rogue's 50");
    assert_eq!(goal(&mut g, 2), Goal::AttackArmy(2), "within a lord's 100");
}

#[test]
fn patrolling_armies_take_targets_only_inside_their_box() {
    let mut s = map();
    let mut lord = army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]);
    lord.patrols = 1;
    lord.patrol_radius = 3;
    s.armies = vec![lord, army(2, (36, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    assert!(matches!(goal(&mut g, 0), Goal::Wander(_)), "the enemy is outside 30 ± 3");
    g.world.armies[1].pos = g.world.map.center((33, 10));
    assert_eq!(goal(&mut g, 0), Goal::AttackArmy(2));
}

#[test]
fn wander_points_are_drawn_x_then_y_in_the_box_or_over_the_map() {
    let mut s = map();
    let mut lord = army(1, (2, 10), 2, ALLY, 0, &[troop(6, 0, 3)]);
    lord.patrols = 1;
    lord.patrol_radius = 3;
    let mut guard = army(3, (40, 5), 2, ALLY, 0, &[troop(4, 0, 1)]);
    guard.patrols = 1;
    guard.patrol_radius = 0;
    s.armies = vec![lord, army(2, (50, 10), 2, ALLY, 0, &[troop(4, 0, 1)]), guard];
    let mut g = start(&s);
    // The patrol box of (2, 10) ± 3 is clamped to columns 0..5 and rows 7..13.
    g.rng = Rng::new(1);
    let mut r = Rng::new(1);
    let drawn: Vec<Tile> = (0..4).map(|_| (r.random(6), 7 + r.random(7))).collect();
    assert_eq!(drawn[3].0, 0, "11478 mod 6");
    // The point in column 0 is lost, as in the original.
    assert_eq!(g.wander_points(0), drawn[..3]);
    assert_eq!(g.rng.state(), r.state(), "eight draws");
    // Not patrolling: anywhere on the 60 × 20 map, no passability test.
    let mut r = g.rng.clone();
    let drawn: Vec<Tile> = (0..4).map(|_| (r.random(60), r.random(20))).collect();
    assert_eq!(g.wander_points(1), drawn.into_iter().filter(|&t| t != (50, 10) && t.0 > 0).collect::<Vec<_>>());
    // A point on the army's own cell is dropped: from state 1 the first is (41 mod 60,
    // 18467 mod 20).
    g.rng = Rng::new(1);
    g.world.armies[1].pos = g.world.map.center((41, 7));
    let points = g.wander_points(1);
    assert_eq!(points.len(), 3);
    assert!(!points.contains(&(41, 7)));
    // A stationary guard never plans, so it draws nothing.
    let before = g.rng.state();
    assert!(g.wander_points(2).is_empty());
    assert_eq!(g.rng.state(), before);
}

#[test]
fn the_goal_is_the_lowest_priority_plus_path_cost() {
    // Two villages with the same tribute; one 4 cells away, one 12: the nearer wins, and the
    // flood's path leads to it.
    let mut s = map();
    for x in [34, 42] {
        let mut v = building(BuildingType::Village, x, 10, (1, 1));
        v.faction = 2;
        v.gold_per_day = 50;
        s.buildings.push(v);
    }
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    let (goal, path) = g.ai_choice(0);
    assert_eq!(goal, Goal::Village(0));
    assert_eq!(path.last(), Some(&(34, 10)));
    assert_eq!(path.len(), 4);
    // Path cost counts: 10 per orthogonal grass cell, 6 for leaving the road of the first
    // village: 40 to the near one, 116 to the far one. A priority 77 lower makes it win.
    let seeds = vec![(100, Goal::Village(0), vec![(34, 10)]), (100 - 77, Goal::Village(1), vec![(42, 10)])];
    assert_eq!(flood(&g.world, (30, 10), &seeds).0, Goal::Village(1));
    let seeds = vec![(100, Goal::Village(0), vec![(34, 10)]), (100 - 75, Goal::Village(1), vec![(42, 10)])];
    assert_eq!(flood(&g.world, (30, 10), &seeds).0, Goal::Village(0));
}

#[test]
fn battle_seeds_follow_the_simulated_battle() {
    let s = |own_left, theirs_left| SimResult { own: 100, own_left, theirs: 50, theirs_left };
    // A clean win costing nothing: (priority + 1) × (relation + 4).
    assert_eq!(battle_seed(s(100, 0), 2, -2, 5, 0), Some(6));
    // Losing half its hit points: 1 + 0.5 × 150 × 2 = 151.
    assert_eq!(battle_seed(s(50, 0), 2, -2, 5, 0), Some((2 + 151) * 2));
    assert_eq!(battle_seed(s(0, 30), 2, -2, 5, 0), None, "a lost battle is no target");
    // Aggression moves the result by a share of both sides' hit points.
    assert_eq!(battle_seed(s(20, 30), 2, -2, 5, 0), None);
    assert!(battle_seed(s(20, 30), 2, -2, 5, 20).is_some(), "a bold army takes a close fight");
    assert_eq!(battle_seed(s(20, 0), 2, -2, 5, -20), None, "a cautious one wants a clear win");
}

#[test]
fn a_hostile_army_in_view_is_attacked_if_it_dares() {
    let mut s = map();
    s.armies = vec![
        army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]),
        army(2, (34, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]),
    ];
    // AIDistance0 of 12 cells for the feudal lord.
    let mut c = content();
    c.options.ai_distance = [12, 6, 3];
    let mut g = start_with(&s, c);
    assert_eq!(goal(&mut g, 0), Goal::AttackArmy(2));
    // The weak one would lose the simulated battle: no target.
    assert_ne!(goal(&mut g, 1), Goal::AttackArmy(1));
    // Out of range: no attack.
    g.world.armies[1].pos = g.world.map.center((45, 10));
    assert_ne!(goal(&mut g, 0), Goal::AttackArmy(2));
    // Aggression makes a weak army bold.
    g.world.armies[1].pos = g.world.map.center((34, 10));
    g.world.armies[1].ai.aggression = 400;
    g.world.armies[1].troops = g.world.armies[0].troops.clone();
    assert_eq!(goal(&mut g, 1), Goal::AttackArmy(1));
}

#[test]
fn flags_remove_goals() {
    let mut s = map();
    let mut v = building(BuildingType::Village, 30, 14, (1, 1));
    v.faction = 2;
    v.gold_per_day = 80;
    s.buildings = vec![v];
    let mut lord = army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]);
    lord.patrols = 1;
    lord.patrol_radius = 5;
    s.armies = vec![lord, army(2, (34, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]), army(3, (28, 10), 2, ALLY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    assert_eq!(goal(&mut g, 0), Goal::AttackArmy(2));
    g.world.armies[0].ai.player_only = true;
    assert_eq!(goal(&mut g, 0), Goal::Village(0), "hunts only the player: the village next");
    g.world.armies[0].ai.player_only = false;
    g.world.armies[1].ai.ignored = true;
    assert_eq!(goal(&mut g, 0), Goal::Village(0), "an army ignored by the AI is no target");
    g.world.armies[0].ai.no_buildings = true;
    assert_eq!(goal(&mut g, 0), Goal::Talk(3), "no interest in buildings: a talk with a friend");
    g.world.armies[0].ai.no_talk = true;
    assert!(matches!(goal(&mut g, 0), Goal::Wander(_)));
    g.world.armies[0].ai.no_random = true;
    assert_eq!(goal(&mut g, 0), Goal::Idle);
}

#[test]
fn peasants_only_wander_but_hunt_the_player() {
    let mut s = map();
    let mut p = army(1, (30, 10), 4, [-2, -2, -2, 3], 2, &[troop(6, 0, 3)]);
    p.patrols = 1;
    p.patrol_radius = 4;
    s.armies = vec![p, army(2, (33, 10), 2, ALLY, 0, &[troop(4, 0, 1)])];
    s.header.heroes[0] = hero(34, 12, 100, &[troop(4, 0, 1)]);
    let mut g = start(&s);
    assert_eq!(goal(&mut g, 0), Goal::AttackPlayer);
    g.pos = g.world.map.center((2, 2));
    assert!(matches!(goal(&mut g, 0), Goal::Wander(_)), "never goes for other armies");
}

#[test]
fn a_friendly_army_hunting_only_the_player_comes_to_meet_him() {
    // A scenario's messenger: friendly, stationary, hunts only the player, no other goals.
    let mut s = map();
    let mut m = army(1, (30, 10), 2, ALLY, 2, &[troop(6, 0, 1)]);
    (m.patrols, m.patrol_radius) = (0, 0);
    (m.hunts_player_only, m.no_random_targets, m.no_socialising) = (1, 1, 1);
    s.armies = vec![m];
    s.header.heroes[0] = hero(38, 10, 100, &[troop(4, 0, 1)]);
    let mut g = start(&s);
    assert!(!g.world.armies[0].hostile());
    assert_eq!(goal(&mut g, 0), Goal::MeetPlayer, "it sets off towards the hero");
    let events = g.wait(12);
    assert!(events.iter().any(|e| matches!(e, crate::rules::game::Event::Met(0))), "they meet: {events:?}");
}

#[test]
fn passive_armies_heal_first_aggressive_ones_attack() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 30, 14, (1, 1));
    castle.faction = 2;
    castle.owner_army = 1;
    s.buildings = vec![castle];
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]), army(2, (33, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    // Passive: healing (Min 0 at full need) beats attacking; the enemy is 3 cells away.
    let mut c = content();
    c.options.ai_targets = AiTargets {
        min_attack_army: Some([2, 2, 90, 40, 40]),
        max_healing: Some([120, 280, 110, 90, 60]),
        min_random: Some([450, 700, 420, 450, 450]),
        ..AiTargets::default()
    };
    let mut g = start_with(&s, c);
    let c = g.content.clone();
    for t in g.world.armies[0].troops.iter_mut() {
        t.hurt = troop_max_hp(&c, t) * 8 / 10;
    }
    g.world.armies[0].ai.model = model::PASSIVE;
    assert_eq!(goal(&mut g, 0), Goal::Heal(0));
    g.world.armies[0].ai.model = model::AGGRESSIVE;
    // Wounded, the lord is weaker; aggression lets him attack anyway.
    g.world.armies[0].ai.aggression = 100;
    assert_eq!(goal(&mut g, 0), Goal::AttackArmy(2));
}

#[test]
fn rogues_go_back_for_their_lost_fort() {
    let mut s = map();
    let mut fort = building(BuildingType::Fort, 50, 10, (1, 1));
    fort.faction = 2;
    s.buildings = vec![fort];
    // A neighbour's rogues (an enemy-faction army would be hostile to the player through the
    // player's −2 towards that faction, world::relation, and go for him instead).
    let mut r = army(1, (10, 10), 3, ENEMY, 1, &[troop(6, 0, 2)]);
    r.home_building = 1;
    s.armies = vec![r];
    let mut g = start(&s);
    assert_eq!(goal(&mut g, 0), Goal::Capture(0), "its home, far out of view");
    // It walks there (40 cells) and takes it (nobody guards it).
    g.wait(48);
    assert_eq!(g.world.locations[0].owner, Owner::Army(1));
    assert_eq!(g.world.locations[0].faction, 3, "the taker's faction");
    assert_eq!(g.ai_stats.captures, 1);
}

#[test]
fn feudal_lords_collect_tribute_and_hoarders_prefer_it() {
    let mut s = map();
    let mut v = building(BuildingType::Village, 32, 12, (1, 1));
    v.faction = 2;
    v.gold_per_day = 10;
    let mut m = building(BuildingType::Market, 28, 12, (1, 1));
    m.faction = 2;
    m.artifact_slots[0] = 7;
    s.buildings = vec![v, m];
    let mut lord = army(1, (30, 10), 2, ALLY, 0, &[troop(4, 0, 2)]);
    lord.gold_income = 400;
    s.armies = vec![lord];
    let mut c = content();
    c.options.ai_targets = AiTargets {
        min_purchase: Some([110, 240, 160, 240, 60]),
        max_purchase: Some([260, 480, 310, 480, 390]),
        gold_purchase: Some([450; 5]),
        min_village: Some([20, 140, 60, 2, 20]),
        max_village: Some([240, 480, 240, 110, 210]),
        gold_village: Some([120; 5]),
        ..AiTargets::default()
    };
    let mut g = start_with(&s, c);
    g.world.armies[0].ai.model = model::TRADING;
    assert_eq!(goal(&mut g, 0), Goal::Shop(1), "a trader shops first");
    g.world.armies[0].ai.model = model::HOARDING;
    assert_eq!(goal(&mut g, 0), Goal::Village(0), "a hoarder collects first");
    let gold = g.world.armies[0].gold;
    g.wait(2);
    assert_eq!(g.world.locations[0].tribute_gold, 0);
    assert_eq!(g.world.armies[0].gold, gold + 10);
}

#[test]
fn a_lord_buys_an_item_one_of_his_units_can_wear() {
    let mut s = map();
    let mut m = building(BuildingType::Market, 32, 10, (1, 1));
    m.faction = 2;
    m.artifact_slots[0] = 9;
    m.artifact_slots[1] = 7;
    s.buildings = vec![m];
    let mut lord = army(1, (30, 10), 2, ALLY, 0, &[troop(4, 0, 1)]);
    lord.gold_income = 300;
    s.armies = vec![lord];
    let mut g = start(&s);
    assert_eq!(goal(&mut g, 0), Goal::Shop(0));
    g.wait(2);
    let a = by_id(&g, 1).unwrap();
    assert_eq!(a.items, vec![ItemId(7)], "the ring, not the potion");
    assert_eq!(a.gold, 300 - 100);
    assert_eq!(g.ai_stats.bought, 1);
    // The ring is worn in battle.
    let units = army_units(&g.content, a);
    assert_eq!(units[0].items.iter().flatten().count(), 1);
}

#[test]
fn noon_pays_income_and_wages_and_hiring_keeps_the_reserve() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 30, 12, (1, 1));
    castle.faction = 2;
    castle.owner_army = 1;
    castle.gold_per_day = 100;
    castle.has_barracks = 1;
    castle.barracks[0] = crate::dt::dtm::RecruitSlot { unit: 4, start_count: 10, max_count: 10 };
    s.buildings = vec![castle];
    let mut lord = army(1, (30, 12), 2, ALLY, 0, &[troop(6, 0, 1)]);
    lord.unknown_80 = 5; // 50 gold a day
    s.armies = vec![lord];
    let mut g = start(&s);
    let c = g.content.clone();
    // Standing in his castle, the lord hires at noon while his gold stays above five days
    // of wages.
    g.world.armies[0].pos = g.world.map.center(g.world.locations[0].tile);
    g.world.armies[0].gold = 200;
    g.wait(4); // 09:00 → 13:00, past the noon
    let a = by_id(&g, 1).unwrap();
    let castle = &g.world.locations[0];
    // Into his army, and into the castle's garrison (half his strength, the default).
    let (army, garrison) = (a.troops.len() - 1, castle.garrison.len());
    assert!(army > 0 && garrison > 0, "hired {army} + {garrison}");
    assert_eq!(castle.recruits[0].stock, Some(10 - (army + garrison) as i32));
    let keep = reserve(&c, a, &a.troops);
    assert!(a.gold >= keep, "kept {} (reserve {keep})", a.gold);
    let mut more = a.troops.clone();
    more.push(a.troops[1]);
    assert!(a.gold - 50 < reserve(&c, a, &more), "one more would eat into the reserve");
    // 200 + 100 castle + 50 own income (byte 80 × 10) − wages at noon − 50 a head.
    let spent = 350 - a.gold - 50 * (army + garrison) as i32;
    assert!((0..=army as i32 * 6).contains(&spent), "wages {spent}");
}

#[test]
fn rogues_pay_no_wages_and_hire_only_rogues() {
    let mut s = map();
    let mut fort = building(BuildingType::Fort, 30, 12, (1, 1));
    fort.faction = 4;
    fort.owner_army = 1;
    fort.has_barracks = 1;
    fort.barracks[0] = crate::dt::dtm::RecruitSlot { unit: 4, start_count: 5, max_count: 5 };
    fort.barracks[1] = crate::dt::dtm::RecruitSlot { unit: 7, start_count: 2, max_count: 2 };
    s.buildings = vec![fort];
    s.armies = vec![army(1, (30, 12), 4, ENEMY, 1, &[troop(6, 0, 1), troop(7, 0, 1)])];
    let mut g = start(&s);
    g.world.armies[0].pos = g.world.map.center(g.world.locations[0].tile);
    g.world.armies[0].gold = 1000;
    g.wait(4);
    let a = by_id(&g, 1).unwrap();
    assert!(a.troops.iter().skip(1).all(|t| t.unit == UnitId(7)), "rogue units only");
    assert_eq!(a.troops.len(), 4, "both rogues in stock");
    assert_eq!(a.gold, 1000 - 2 * 60, "no wages");
}

#[test]
fn unpaid_lords_lose_units() {
    let mut s = map();
    s.armies = vec![army(1, (30, 12), 2, ALLY, 0, &[troop(6, 0, 1), troop(6, 0, 2)])];
    let mut g = start(&s);
    g.world.armies[0].gold = 0;
    g.world.armies[0].ai.extra_income = 0;
    g.wait(24 * 8);
    assert_eq!(by_id(&g, 1).unwrap().troops.len(), 2, "one left after seven unpaid days");
}

#[test]
fn hostile_armies_that_meet_fight_and_the_loser_leaves() {
    let mut s = map();
    let mut weak = army(2, (31, 10), 4, ENEMY, 1, &[troop(4, 0, 1)]);
    weak.gold_income = 80;
    weak.artifacts = [7, 0, 0];
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]), weak];
    let mut g = start(&s);
    g.world.armies[0].gold = 0;
    g.world.armies[0].ai.extra_income = 0;
    let events = g.wait(1);
    assert_eq!(g.ai_stats.battles, 1);
    assert!(by_id(&g, 2).is_none(), "the loser is off the map");
    let w = by_id(&g, 1).unwrap();
    assert_eq!(w.gold, 40, "half the loser's gold");
    assert_eq!(w.items, vec![ItemId(7)]);
    assert!(w.troops.iter().any(|t| t.xp > 0 || t.level > 1), "the winners gained experience");
    assert!(g.army_beaten_by_anyone(2));
    assert!(!g.beaten_armies.contains(&2), "not by the player");
    assert!(events.iter().all(|e| !matches!(e, Event::Battle(_))), "far from the hero: not reported");
    assert!(g.ai_log.is_empty());
}

#[test]
fn battles_within_sight_are_reported() {
    let mut s = map();
    s.header.heroes[0] = hero(30, 14, 100, &[troop(4, 0, 1)]);
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]), army(2, (31, 10), 4, ENEMY, 1, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    let events = g.wait(1);
    assert!(events.iter().any(|e| matches!(e, Event::Battle(n) if n.text.contains("defeated"))), "{events:?}");
    assert_eq!(g.ai_log.len(), 1);
}

#[test]
fn an_ai_army_storms_a_hostile_fort_and_takes_it() {
    let mut s = map();
    let mut fort = building(BuildingType::Fort, 34, 10, (1, 1));
    fort.faction = 4;
    fort.garrison[0] = troop(4, 0, 1);
    fort.gold_per_day = 70;
    s.buildings = vec![fort];
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)])];
    let mut g = start(&s);
    assert_eq!(goal(&mut g, 0), Goal::Capture(0));
    g.wait(6);
    let f = &g.world.locations[0];
    assert_eq!((f.owner, f.faction), (Owner::Army(1), 2));
    assert!(!f.garrison.is_empty() && f.garrison.iter().all(|t| t.unit == UnitId(6)), "it leaves some of its own troops there");
    assert!(by_id(&g, 1).unwrap().troops.len() < 3);
    assert_eq!((g.ai_stats.battles, g.ai_stats.captures), (1, 1));
    // Its income goes to the lord from now on.
    let gold = by_id(&g, 1).unwrap().gold;
    g.wait(24);
    assert!(by_id(&g, 1).unwrap().gold >= gold + 70 - army_wages(&g.content, &by_id(&g, 1).unwrap().troops));
}

#[test]
fn the_players_castle_can_be_lost_and_he_hears_of_it() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 34, 10, (1, 1));
    castle.faction = 1;
    castle.gold_per_day = 100;
    s.buildings = vec![castle];
    s.armies = vec![army(1, (30, 10), 4, [-2, -2, 1, 3], 0, &[troop(6, 0, 3)])];
    let mut g = start(&s);
    assert!(g.world.locations[0].owned());
    let income = g.daily_income();
    let mut events = Vec::new();
    for _ in 0..6 {
        events.extend(g.wait(1));
    }
    assert_eq!(g.world.locations[0].owner, Owner::Army(1));
    assert!(g.daily_income() < income);
    assert!(events.iter().any(|e| matches!(e, Event::Battle(n) if n.text.contains("took"))), "{events:?}");
}

#[test]
fn a_beaten_lord_retreats_to_his_castle_and_returns() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 20, 10, (1, 1));
    castle.faction = 4;
    castle.owner_army = 2;
    s.buildings = vec![castle];
    let mut lord = army(2, (31, 10), 4, ENEMY, 0, &[troop(4, 0, 1), troop(4, 0, 2)]);
    lord.home_building = 1;
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]), lord];
    let mut g = start(&s);
    g.wait(1);
    assert!(by_id(&g, 2).is_none());
    assert_eq!(g.world.respawns.len(), 1);
    assert!(g.world.respawns[0].lord);
    assert_eq!(g.world.respawns[0].army.troops.len(), 1, "the leader alone");
    assert_eq!(g.ai_stats.retreats, 1);
    // Keep the victor away from the castle and the lord.
    g.world.armies[0].ai.no_buildings = true;
    g.world.armies[0].ai.no_random = true;
    g.world.armies[0].ai.player_only = true;
    g.wait(24 * RECOVER_DAYS as u32 + 2);
    let back = by_id(&g, 2).expect("back from his castle");
    assert!(back.troops.iter().all(|t| t.hurt == 0), "recovered");
    assert_eq!(g.world.locations[0].owner, Owner::Army(2));
}

#[test]
fn a_lord_without_buildings_falls_for_good() {
    let mut s = map();
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]), army(2, (31, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    g.wait(1);
    assert!(by_id(&g, 2).is_none());
    assert!(g.world.respawns.is_empty());
}

#[test]
fn armies_respawn_at_the_centre_of_their_home_after_their_days() {
    for whole in [false, true] {
        let mut s = map();
        // Its home: a 3 × 3 village whose centre is (41, 5).
        s.buildings = vec![building(BuildingType::Village, 42, 6, (3, 3))];
        let mut gang = army(2, (31, 10), 4, ENEMY, 1, &[troop(4, 0, 1), troop(5, 0, 2)]);
        gang.respawn_days = 2;
        gang.respawn_all = whole as u8;
        gang.home_building = 1;
        gang.gold_income = 10;
        gang.unknown_80 = 3;
        s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]), gang];
        let mut g = start(&s);
        g.wait(1);
        assert!(by_id(&g, 2).is_none());
        // The victor stays put so it does not beat the gang again at once.
        g.world.armies[0].ai.player_only = true;
        g.world.armies[0].ai.no_buildings = true;
        g.wait(47);
        assert!(by_id(&g, 2).is_none(), "not yet");
        for _ in 0..4 {
            if by_id(&g, 2).is_some() {
                break;
            }
            g.wait(1);
        }
        let back = by_id(&g, 2).expect("respawned");
        assert_eq!(back.troops.len(), if whole { 3 } else { 1 });
        assert!(back.troops.iter().all(|t| t.hurt == 0), "at full health");
        // It came back at the centre and may have taken a step or two since.
        assert!(g.world.map.distance(back.tile(&g.world.map), (41, 5)) <= 2, "at the centre of its home: {:?}", back.tile(&g.world.map));
        assert!(back.gold >= 2 * 30, "the days' income: {}", back.gold);
        assert_eq!(g.world.locations[0].owner, Owner::Army(2), "a rogue takes its village");
        assert_eq!(g.ai_stats.respawns, 1);
    }
    // No home building: no respawn.
    let mut s = map();
    let mut gang = army(2, (31, 10), 4, ENEMY, 1, &[troop(4, 0, 1)]);
    gang.respawn_days = 1;
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(6, 0, 3)]), gang];
    let mut g = start(&s);
    g.wait(1);
    assert!(by_id(&g, 2).is_none() && g.world.respawns.len() == 1);
    g.world.armies[0].ai.player_only = true;
    g.wait(30);
    assert!(by_id(&g, 2).is_none() && g.world.respawns.is_empty(), "it does not come back");
}

#[test]
fn the_player_beating_a_lord_sends_him_home_too() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 20, 10, (1, 1));
    castle.faction = 4;
    castle.owner_army = 1;
    s.buildings = vec![castle];
    s.armies = vec![army(1, (5, 5), 4, [-2, -2, 1, 3], 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    g.foe = Some(Foe::Army(0));
    let mut b = g.start_battle();
    b.begin();
    for f in b.fighters.iter_mut().filter(|f| f.team == Team::Enemy) {
        f.hp = 0;
    }
    g.resolve_battle(&b);
    assert!(g.world.armies.is_empty());
    assert!(g.beaten_armies.contains(&1) && g.army_beaten_by_anyone(1));
    assert_eq!(g.world.respawns.len(), 1);
}

#[test]
fn write_back_keeps_the_leader_while_his_army_lives() {
    let c = content();
    let slot = |k| crate::rules::formation::Slot::new(crate::rules::formation::Row::Front, k);
    let mut troops = vec![Troop::new(UnitId(6), 1, slot(0)), Troop::new(UnitId(4), 1, slot(1)), Troop::new(UnitId(4), 1, slot(2))];
    write_back(&c, &mut Rng::new(1), &mut troops, &[(0, 120), (20, 50), (0, 50)], &[0, 1000, 0]);
    assert_eq!(troops.len(), 2, "the dead warrior leaves");
    assert_eq!(troops[0].hurt, 119, "the leader is back with 1 HP");
    assert!(troops[1].level > 1, "1000 XP levels up");
    // Everybody dead: the army keeps its troops for the caller (the leader retreats).
    let mut troops = vec![Troop::new(UnitId(6), 1, slot(0))];
    write_back(&c, &mut Rng::new(1), &mut troops, &[(0, 120)], &[0]);
    assert_eq!(troops.len(), 1);
}

/// Content whose unit 9 can become 10 (slot 1) or 11 (slot 3), militia 4 guard 5 (slot 1)
/// only, unit 8 squire 6 (slot 1) only.
fn tree_content() -> Content {
    use crate::rules::content::Upgrade;
    let up = |target: u32, slot: u8, level: i32| Upgrade { target_name: String::new(), target: Some(target), level, slot };
    let mut units = vec![
        ck::warrior(1, 20, 5),
        ck::warrior(4, 10, 2),
        ck::warrior(5, 12, 3),
        ck::warrior(6, 14, 3),
        ck::warrior(8, 12, 2),
        ck::warrior(9, 10, 2),
        ck::warrior(10, 15, 3),
        ck::warrior(11, 15, 3),
    ];
    units[1].upgrades = vec![up(5, 1, 1)];
    units[4].upgrades = vec![up(6, 1, 1)];
    units[5].upgrades = vec![up(10, 1, 1), up(11, 3, 1)];
    ck::content(units, vec![])
}

#[test]
fn ai_units_keep_xp_and_take_the_upgrade_tree() {
    let c = tree_content();
    let slot = crate::rules::formation::Slot::new(crate::rules::formation::Row::Front, 0);
    // Banking: 60 needed, 70 leaves 10; no promotion before the level.
    let mut t = Troop::new(UnitId(9), 1, slot);
    assert_eq!(troop_gain_xp(&c, &mut t, 50), 0);
    assert_eq!((t.level, t.xp), (1, 50));
    // A level reached: a random filled option, level 1 and no XP in the new class.
    let mut picks = std::collections::BTreeMap::new();
    let mut rng = Rng::new(3);
    for _ in 0..60 {
        let mut t = Troop::new(UnitId(9), 1, slot);
        ai_unit_gain(&c, &mut rng, &mut t, 70);
        assert_eq!((t.level, t.xp), (1, 0));
        *picks.entry(t.unit.0).or_insert(0) += 1;
    }
    assert_eq!(picks.keys().copied().collect::<Vec<_>>(), vec![10, 11], "both branches");
    // Militia: option 1 one time in three, option 3 (empty) otherwise; unit 8 the reverse.
    let (mut militia, mut squire) = (0, 0);
    for _ in 0..300 {
        let mut m = Troop::new(UnitId(4), 1, slot);
        ai_unit_gain(&c, &mut rng, &mut m, 60);
        militia += i32::from(m.unit == UnitId(5));
        let mut q = Troop::new(UnitId(8), 1, slot);
        ai_unit_gain(&c, &mut rng, &mut q, 60);
        squire += i32::from(q.unit == UnitId(6));
    }
    assert!((70..130).contains(&militia), "{militia}/300");
    assert!((170..230).contains(&squire), "{squire}/300");
}

#[test]
fn ai_hires_start_with_bonus_xp_and_the_players_experience() {
    let mut s = map();
    let mut c1 = building(BuildingType::Castle, 30, 12, (1, 1));
    c1.faction = 4;
    c1.owner_army = 1;
    c1.has_barracks = 1;
    c1.barracks[0] = crate::dt::dtm::RecruitSlot { unit: 4, start_count: 5, max_count: 5 };
    s.buildings = vec![c1];
    let mut a = army(1, (30, 12), 4, [-2, -2, 1, 3], 0, &[troop(6, 0, 1)]);
    a.hire_bonus_exp = 100;
    s.armies = vec![a];
    let mut g = start(&s);
    g.world.armies[0].gold = 10_000;
    let l = 0;
    assert!(g.ai_hire_at(0, l) > 0);
    let hired: Vec<&Troop> = g.world.armies[0].troops.iter().skip(1).collect();
    // 100 bonus: a random 50..=149, over the 60 of a first level: level 2 (or 1 with 50–59).
    assert!(hired.iter().all(|t| (t.level == 2 && t.xp < 84) || (t.level == 1 && t.xp >= 50)), "{hired:?}");
    // Like the player: the player's army's strength and XP per unit join the bonus.
    g.world.armies[0].troops.truncate(1);
    g.world.armies[0].ai.hire_bonus_exp = 0;
    g.world.armies[0].ai.exp_like_player = true;
    g.world.locations[l].recruits[0].stock = Some(5);
    g.squad[0].xp = 5000;
    assert!(g.ai_hire_at(0, l) > 0);
    assert!(g.world.armies[0].troops.iter().skip(1).all(|t| t.level > 2), "{:?}", g.world.armies[0].troops);
}


#[test]
fn routes_are_planned_once_per_goal() {
    let mut s = map();
    let mut p = army(1, (30, 10), 2, ALLY, 0, &[troop(4, 0, 1)]);
    p.patrols = 1;
    p.patrol_radius = 12;
    s.armies = vec![p];
    let mut g = start(&s);
    g.wait(24);
    // A patrol leg is one search, not one per slice (288 slices in a day).
    assert!(g.ai_stats.paths < 40, "{} searches", g.ai_stats.paths);
    assert!(g.ai_stats.paths > 0);
}

#[test]
fn hostile_armies_still_chase_the_hero() {
    let mut s = map();
    s.header.heroes[0] = hero(30, 14, 100, &[troop(4, 0, 1)]);
    // Strong enough to win the simulated battle against him.
    s.armies = vec![army(1, (30, 9), 4, [-2, -2, 1, 3], 1, &[troop(6, 0, 3)])];
    let mut g = start(&s);
    let events = g.wait(4);
    assert!(events.iter().any(|e| matches!(e, Event::Encounter(0))), "{events:?}");
}

#[test]
fn armies_walk_around_buildings_that_are_not_theirs_or_friends() {
    let mut s = map();
    let mut fort = building(BuildingType::Fort, 40, 10, (1, 1));
    fort.faction = 4;
    let mut ally = building(BuildingType::Fort, 40, 4, (1, 1));
    ally.faction = 3;
    s.buildings = vec![fort, ally];
    s.armies = vec![army(1, (30, 10), 2, ALLY, 0, &[troop(4, 0, 1)])];
    let g = start(&s);
    let (a, w) = (&g.world.armies[0], &g.world);
    assert!(bars_army(a, &w.locations[0]), "an enemy fort");
    assert!(!bars_army(a, &w.locations[1]), "a friend's fort (relation 1)");
    let covers = |t: Tile, l: usize| w.location_covering(t) == Some(l);
    let fort_cells: Vec<Tile> = (0..60).flat_map(|x| (0..20).map(move |y| (x, y))).filter(|&t| covers(t, 0)).collect();
    assert!(!fort_cells.is_empty());
    let (left, right) = ((fort_cells.iter().map(|t| t.0).min().unwrap() - 2, 10), (fort_cells.iter().map(|t| t.0).max().unwrap() + 2, 10));
    assert!(w.map.path(left, right).iter().any(|&t| covers(t, 0)), "the straight way crosses it");
    let across = army_path(w, a, left, right, usize::MAX);
    assert!(!across.is_empty() && across.iter().all(|&t| !covers(t, 0)), "{across:?}");
    // The fort it heads for stays open.
    let into = army_path(w, a, left, fort_cells[0], usize::MAX);
    assert_eq!(into.last(), Some(&fort_cells[0]));
    // Its own faction's fort does not stand in its way.
    let mut s2 = s.clone();
    s2.buildings[0].faction = 2;
    let g2 = start(&s2);
    assert!(!bars_army(&g2.world.armies[0], &g2.world.locations[0]));
}
