//! The AI on small synthetic scenarios, from the numbers of `original-mechanics/ai.md`.
use std::sync::Arc;

use super::*;
use crate::dt::data::AiTargets;
use crate::dt::dtm::{Army as DtArmy, BuildingType, RecruitSlot, Scenario, Troop as DtTroop};
use crate::rules::content::testkit as ck;
use crate::rules::content::{ArtefactType, HeroClass, Stat, StatMods, UnitDef};
use crate::rules::formation::{Row, Slot};
use crate::rules::world::testkit::{self as tk, building, hero, scenario, troop};

/// Units 1–3 heroes, 4 a weak warrior (cost 50), 5 a shooter, 6 a strong warrior (cost 200,
/// 120 HP), 7 a rogue warrior; item 7 a ring, 9 a potion, 11 a ring of +10 attack, 12 a ring
/// of +6 attack, 13 an amulet of +10 attack.
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
    let ring = |id, atk| crate::rules::content::ArtefactDef { add: StatMods::from([(Stat::AttackBlow, atk)]), ..ck::item(id, ArtefactType::Ring) };
    let amulet = crate::rules::content::ArtefactDef { add: StatMods::from([(Stat::AttackBlow, 10)]), ..ck::item(13, ArtefactType::Amulet) };
    ck::content(units, vec![ck::item(7, ArtefactType::Ring), ck::item(9, ArtefactType::Potion), ring(11, 10), ring(12, 6), amulet])
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

fn slot(col: u8) -> Slot {
    Slot::new(Row::Front, col)
}

fn sim(own: i64, own_left: i64, theirs: i64, theirs_left: i64) -> SimResult {
    SimResult { own, own_left, theirs, theirs_left, turn: 3 }
}

/// Army `i` plans now, as at an arrival (its distances to every other party).
fn plan(g: &mut Game, i: usize) {
    let here = g.world.armies[i].tile(&g.world.map);
    let grid = g.world.map.grid;
    let dist: Vec<(Party, i32)> = g.parties().into_iter().filter(|&p| p != Party::Army(i)).map(|p| (p, grid.octile(here, g.cell_of(p)))).collect();
    let hero = HeroCells { cells: [Some(g.tile()), None], at: g.tile() };
    g.ai_plan(i, &dist, &hero);
}

// ----------------------------------------------------------------------------------------
// Profile, relation, scores
// ----------------------------------------------------------------------------------------

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
fn priorities_by_target_model_a_missing_key_reads_0() {
    let mut o = GlobalOptions::default();
    assert_eq!(Priorities::of(&o, 1), Priorities::default(), "no _Global.ini: all 0, as the exe reads it");
    o.ai_targets = AiTargets { min_attack_army: Some([1, 2, 3, 4, 5]), min_random: Some([10, 20, 30, 40, 50]), max_healing: Some([100, 200, 300, 400, 500]), ..AiTargets::default() };
    let p = Priorities::of(&o, model::PASSIVE);
    assert_eq!((p.attack_army, p.random, p.heal), (3, 30, (0, 300)), "the misspelt minimum reads 0");
    assert_eq!([Style::Feudal, Style::Rogue, Style::Peasant].map(|s| target_range(&o, s)), [100, 50, 25], "AIDistance by style");
}

#[test]
fn the_relation_is_the_originals_two_sided_rule() {
    // 0x4a0868: a = my attitude to his faction, b = his to mine.
    let r = |a: (u8, [i8; 4]), b: (u8, [i8; 4])| relation_between((a.0, &a.1), (b.0, &b.1));
    assert_eq!(r((2, [0, 0, 0, 3]), (4, [0, 1, 0, 0])), (3 + 1) / 2, "both ≥ 0: their mean");
    assert_eq!(r((2, [0, 0, 0, -2]), (4, [0, 3, 0, 0])), -2, "mine below 0: mine");
    assert_eq!(r((2, [0, 0, 0, 3]), (4, [0, -3, 0, 0])), -1, "only his below 0: −1");
    // The factions are not compared: an army ill-disposed to its own faction is hostile to it.
    assert_eq!(r((3, [0, 0, -1, 0]), (3, [0, 0, 2, 0])), -1);
}

#[test]
fn the_army_score_follows_the_simulated_battle() {
    // 0x4a08f8 with AtackArmy 10, ZeroDensity 5, both speeds 5, BattleEndTurn 25.
    let score = |s: SimResult, g: i32, r: i8| army_score(s, g, r, 10, 5, 5, 5, 25);
    // A win without loss: Round(A0/B0 + 1), halves to even: 100/200 + 1 = 1.5 → 2.
    assert_eq!(score(sim(100, 100, 50, 0), 0, 0), 3);
    assert_eq!(score(sim(100, 100, 200, 0), 0, 0), 2);
    // A win at a loss: Round((1 − A1/A0)·30·ZD·A0/B0 + 1) = Round(0.5·150 + 1).
    assert_eq!(score(sim(100, 50, 100, 0), 0, 0), 76);
    // By relation: div (r + 1) when friendly, (AtackArmy + s)·(r + 4) when hostile.
    assert_eq!(score(sim(100, 100, 50, 0), 0, 1), 1);
    assert_eq!(score(sim(100, 100, 50, 0), 0, -2), (10 + 3) * 2);
    assert_eq!(score(sim(100, 100, 50, 0), 0, -3), 10 + 3);
    // A loss: −5 − Round(√(B0/A0)·ZD·speedA/speedC), then ((2 − r)·s) div 3 when hostile.
    assert_eq!(score(sim(100, 0, 400, 300), 0, 0), -15);
    assert_eq!(score(sim(100, 0, 400, 300), 0, -3), -25);
    assert_eq!(score(sim(100, 0, 400, 300), 0, -1), -15);
    assert_eq!(score(sim(100, 0, 1_000_000, 300), 0, 0), -50, "not below −50");
    assert_eq!(score(sim(100, 0, 1_000_000, 300), 0, -3), -83, "the hostile scaling after the floor");
    // A slower army fears a fast enemy more.
    assert_eq!(army_score(sim(100, 0, 400, 300), 0, 0, 10, 5, 10, 5, 25), -25);
    // Nothing happened, or the turn limit was reached (a win on that turn too): 0.
    assert_eq!(score(sim(100, 100, 50, 50), 0, -2), 0);
    assert_eq!(army_score(SimResult { turn: 25, ..sim(100, 100, 50, 0) }, 0, -2, 10, 5, 5, 5, 25), 0);
    // Aggression shifts both: B1 −= Round(g·B0/100), A1 += Round(g·A0/100).
    assert_eq!(score(sim(100, 20, 100, 60), 0, 0), -10, "a loss as it stands");
    assert_eq!(score(sim(100, 20, 100, 60), 50, 0), 46, "B1 = 10, A1 = 70: Round(0.3·150 + 1)");
    // A negative one: a tenth on its own side (÷1000), the full shift on the other's.
    assert_eq!(score(sim(100, 60, 100, 50), -50, 0), -10, "A1 = 60 − 5 = 55 < B1 = 50 + 50");
    // The shift can lift A1 to A0: then the no-loss formula.
    assert_eq!(score(sim(100, 80, 100, 0), 50, 0), 2);
}

#[test]
fn a_repulsion_cone_has_the_originals_box() {
    let field = |w: i32, at: Tile, f: f32, s: i32| {
        let mut m = vec![1u16; (w * w) as usize];
        repulsion(&mut m, w, w, at, f, s);
        m
    };
    let get = |m: &[u16], (x, y): Tile| m[(y * 10 + x) as usize];
    // Strength 2, slope 1: r = 0, the box is the cell up-left of the centre, whose value
    // 2 − floor(1.5) = 1 is not above 1: nothing.
    assert!(field(10, (5, 5), 1.0, 2).iter().all(|&v| v == 1));
    // Strength 3: r = 2, columns 2..=6 and rows 2..=6.
    let m = field(10, (5, 5), 1.0, 3);
    assert_eq!([get(&m, (5, 5)), get(&m, (4, 5)), get(&m, (4, 4)), get(&m, (6, 6)), get(&m, (3, 5)), get(&m, (7, 5))], [1 + 3, 1 + 2, 1 + 2, 1 + 2, 1, 1]);
    // A guard's slope 5 needs strength 5, the steep slope 25 strength 15.
    assert!(field(10, (5, 5), 5.0, 4).iter().all(|&v| v == 1));
    assert_eq!(get(&field(10, (5, 5), 5.0, 5), (5, 5)), 1 + 5);
    assert!(field(10, (5, 5), 25.0, 14).iter().all(|&v| v == 1));
    assert_eq!(get(&field(10, (5, 5), 25.0, 15), (5, 5)), 1 + 15);
    // At the map's corner the clamp brings the centre back in.
    assert_eq!(get(&field(10, (0, 0), 1.0, 2), (0, 0)), 1 + 2);
}

#[test]
fn a_seed_one_above_an_earlier_one_overwrites_its_cell() {
    // 0x482984: a new seed is refused only when one already there is lower by 2 or more.
    let mut s = scenario(10, 10);
    s.header.heroes[0] = hero(0, 0, 0, &[]);
    let g = start(&s);
    let m = &g.world.map;
    let ones = vec![1u16; 100];
    let at = |f: &crate::rules::map::FloodField, t: Tile| f.dist[(t.1 * 10 + t.0) as usize];
    let f = m.flood_maps(m.land_costs(), &ones, &[((5, 5), 10), ((5, 5), 11)], (9, 9));
    assert_eq!(at(&f, (5, 5)), 12, "the later seed's 11 + 1");
    assert_eq!(at(&f, (6, 5)), 11 + 5 * 2, "the earlier one still expands from its own 11");
    let f = m.flood_maps(m.land_costs(), &ones, &[((5, 5), 10), ((5, 5), 12)], (9, 9));
    assert_eq!(at(&f, (5, 5)), 11, "two above: refused");
    // A seed on the walker's own cell is dropped.
    let f = m.flood_maps(m.land_costs(), &ones, &[((9, 9), 1), ((2, 2), 1)], (9, 9));
    assert_eq!(f.kept, 1);
}

#[test]
fn totals_and_spare_gold_as_the_original() {
    // Leader unit 6 (no wage, Round(200/2) = 100 untiered) and two unit 4s (wage
    // Round(50/2 × 0.25) = 6, Round(50/2) = 25 each): W = (12 + 2·150) div 3 = 104.
    let mut s = map();
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 2)]);
    a.leader_unit = 6;
    s.armies = vec![a];
    let mut g = start(&s);
    let t = g.army_totals(0);
    assert_eq!((t.wages, t.recruit_sum), (12, 150));
    let a = &mut g.world.armies[0];
    (a.gold, a.mind.income, a.mind.village_avg) = (1000, 20, 50);
    // Feudal: gold − 5·W + (income + average)·4.
    assert_eq!(g.spare_gold(0), 1000 - 5 * 104 + 70 * 4);
    g.world.armies[0].gold = 100;
    assert_eq!(g.spare_gold(0), 0, "not below 0");
    // Rogue: one day's worth, not above its gold.
    g.world.armies[0].ai.style = Style::Rogue;
    g.world.armies[0].gold = 1000;
    assert_eq!(g.spare_gold(0), 1000 - 104 + 70);
    g.world.armies[0].mind.income = 5000;
    assert_eq!(g.spare_gold(0), 1000);
    // A wounded unit's heal bill counts its current HP: 25 of 50 → Round(25·50/50/2) = 12.
    let c = g.content.clone();
    g.world.armies[0].troops[1].hurt = 25;
    g.world.armies[0].troops[2].died_at = Some(1);
    let t = g.army_totals(0);
    assert_eq!((t.heal_bill, t.res_bill, t.missing), (12, 150, 25), "the dead unit: Round(50 × 300/100)");
    assert_eq!(t.wages, 6, "a corpse draws no wage");
    assert_eq!(t.strength, g.world.armies[0].troops.iter().map(|tr| tactical_modes(&c, tr, 0).1).sum::<i32>(), "the dead count in the strength");
}

/// The priorities of the shipped file for target model 0 (only those used here).
fn priorities() -> AiTargets {
    AiTargets {
        min_attack_army: Some([10; 5]),
        min_attack_castle: Some([10; 5]),
        min_random: Some([1000; 5]),
        min_talking: Some([200; 5]),
        max_healing: Some([100; 5]),
        min_garrison: Some([50; 5]),
        max_garrison: Some([250; 5]),
        min_purchase: Some([100; 5]),
        max_purchase: Some([300; 5]),
        gold_purchase: Some([2; 5]),
        min_village: Some([10; 5]),
        max_village: Some([110; 5]),
        gold_village: Some([100; 5]),
        ..AiTargets::default()
    }
}

fn with_priorities() -> Content {
    let mut c = content();
    c.options.ai_targets = priorities();
    c
}

#[test]
fn a_villages_score_by_its_stock_and_a_rogue_thrice() {
    // Round((1 − stock/(GoldVillage + spare))·(Max − Min)), then + Min (Min when below 0).
    let mut s = map();
    s.buildings = vec![building(BuildingType::Village, 40, 10, (1, 1))];
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start_with(&s, with_priorities());
    g.world.armies[0].gold = 0;
    g.world.locations[0].tribute_gold = 30;
    assert_eq!(g.building_score(0, 0), 70 + 10);
    g.world.armies[0].ai.style = Style::Rogue;
    assert_eq!(g.building_score(0, 0), 80 * 3);
    g.world.armies[0].ai.style = Style::Feudal;
    g.world.locations[0].tribute_gold = 300;
    assert_eq!(g.building_score(0, 0), 10, "(1 − 3)·100 < 0: the minimum");
    g.world.locations[0].tribute_gold = 0;
    assert_eq!(g.building_score(0, 0), 0, "no stock: no interest");
}

#[test]
fn a_castles_attack_score_and_what_forbids_it() {
    // AtackCastle 10: Round(10·50/(income + 1)); an empty garrison: div 4 + 1; no income
    // and no garrison: ×50.
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 40, 10, (1, 1));
    castle.faction = 2;
    castle.relations = [1, 3, 1, -2];
    s.buildings = vec![castle];
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(6, 0, 3)])];
    let mut g = start_with(&s, with_priorities());
    assert_eq!(g.building_score(0, 0), (500 / 4 + 1) * 50);
    g.world.locations[0].gold_income = 9;
    assert_eq!(g.building_score(0, 0), 50 / 4 + 1);
    // A garrison it beats: + Round(((1 − A1/A0) + B1/B0)·30·ZD + 1), at least 1.
    g.world.locations[0].garrison = vec![Troop::new(UnitId(4), 1, slot(2))];
    let v = g.building_score(0, 0);
    assert!(v > 50 && v < 50 + 152, "{v}");
    // One it cannot beat forbids it: −1.
    g.world.locations[0].garrison = (0..6).map(|k| Troop::new(UnitId(6), 3, slot(k))).collect();
    g.world.armies[0].troops.truncate(1);
    assert_eq!(g.building_score(0, 0), -1);
    // A town only at attitude −3, and taverns, churches, smithies, obelisks never.
    for (kind, att, scored) in [(LocationKind::Town, -2, false), (LocationKind::Town, -3, true), (LocationKind::Tavern, -3, false)] {
        let l = &mut g.world.locations[0];
        (l.kind, l.garrison) = (kind, Vec::new());
        g.world.armies[0].ai.relations[1] = att;
        assert_eq!(g.building_score(0, 0) > 0, scored, "{kind:?} at {att}");
    }
    // Peasants never attack.
    g.world.locations[0].kind = LocationKind::Castle;
    g.world.armies[0].ai.style = Style::Peasant;
    assert_eq!(g.building_score(0, 0), 0);
}

#[test]
fn a_stationary_guard_it_cannot_beat_forbids_a_building() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 40, 10, (1, 1));
    castle.faction = 2;
    s.buildings = vec![castle];
    let mut guard = army(2, (40, 10), 2, ALLY, 0, &[troop(6, 4, 6)]);
    guard.patrols = 1;
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]), guard];
    let mut g = start_with(&s, with_priorities());
    assert_eq!(g.world.armies[1].mind.standing, Some(0));
    assert_eq!(g.building_score(0, 0), -1);
    // Gone, the castle is a target again.
    g.world.armies[1].mind.standing = None;
    assert!(g.building_score(0, 0) > 0);
}

#[test]
fn a_garrison_part_and_the_smallest_positive_part_wins() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 40, 10, (1, 1));
    castle.faction = 4;
    castle.owner_army = 1;
    s.buildings = vec![castle];
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start_with(&s, with_priorities());
    let c = g.content.clone();
    let strength = g.army_totals(0).strength;
    // An empty garrison: G = 0 < S: Round(200·0/S) + 50 = 50.
    assert_eq!(g.building_score(0, 0), 50);
    // Equal strengths: 0, no interest.
    g.world.locations[0].garrison = g.world.armies[0].troops.clone();
    assert_eq!(totals(&c, &g.world.locations[0].garrison, 0).strength, strength);
    assert_eq!(g.building_score(0, 0), 0);
    // Its own castle's stock counts too: the village part, smaller, wins.
    g.world.locations[0].garrison.clear();
    g.world.locations[0].tribute_gold = 90;
    g.world.armies[0].gold = 0;
    assert_eq!(g.building_score(0, 0), 20, "Round(0.1·100) + 10 < 50");
    // Bridges score 0.
    g.world.locations[0].kind = LocationKind::WoodenBridge;
    assert_eq!(g.building_score(0, 0), 0);
}

#[test]
fn a_purchase_score_from_the_barracks_and_the_goods() {
    // Recruits of its leader's Nature: (Σ count·Cost div Σ count) × min(Σ count, 12 −
    // units) = 50 × 3; then Round(Min·Gold/V + Max·Gold/spare).
    let mut s = map();
    let mut town = building(BuildingType::Market, 40, 10, (1, 1));
    town.faction = 3;
    town.relations = [1, 0, 1, 1];
    town.has_barracks = 1;
    town.barracks[0] = RecruitSlot { unit: 4, start_count: 3, max_count: 5 };
    town.barracks[1] = RecruitSlot { unit: 7, start_count: 5, max_count: 5 };
    s.buildings = vec![town];
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start_with(&s, with_priorities());
    g.world.locations[0].shop = None;
    g.world.armies[0].gold = 10_000;
    let spare = g.spare_gold(0);
    assert_eq!(g.building_score(0, 0), delphi_round(100.0 * 2.0 / 150.0 + 300.0 * 2.0 / spare as f64) as i32);
    // No spare gold, or a building ill-disposed to it: nothing to buy.
    g.world.locations[0].relations[3] = -1;
    assert_eq!(g.building_score(0, 0), 0);
}

#[test]
fn a_stationary_guard_does_nothing_at_all() {
    let mut s = map();
    let mut guard = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    guard.patrols = 1;
    s.armies = vec![guard];
    let mut g = start(&s);
    let before = g.world.armies[0].clone();
    g.wait(48);
    let a = &g.world.armies[0];
    assert_eq!((a.pos, a.budget, a.mind.wander, a.mind.next_noon, a.gold), (before.pos, 0.0, before.mind.wander, before.mind.next_noon, before.gold), "no step, plan or noon");
}

#[test]
fn an_army_with_nothing_to_do_steps_in_place() {
    // No targets: each cost × speed of its own cell (grass 5 × 5) is an arrival with an idle
    // plan; the countdown is left as it is.
    let mut s = map();
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    a.no_random_targets = 1;
    a.no_building_interest = 1;
    s.armies = vec![a];
    let mut g = start(&s);
    g.world.armies[0].mind.scores.insert(HERO, 0);
    g.wait(1);
    // Hostile to the hero: its score of 0 seeds nothing either. 60 minutes: two arrivals.
    let a = &g.world.armies[0];
    assert_eq!((a.mind.idle, a.budget, a.path.len(), a.tile(&g.world.map)), (2, 10.0, 0, (30, 10)));
    assert_eq!(g.ai_stats.paths, 2, "it planned at each");
}

#[test]
fn a_step_in_place_barred_by_the_hero_follows_the_originals_path_index() {
    // 0x4a399c: a step the hero's cell bars leaves the path index where it is. On a one-cell
    // path read at index 0 that is a step (the countdown −1, no idle plan); with no path at
    // all, or after a plan with no seed once the index had moved on, it is past the end: an
    // idle plan.
    let mut s = map();
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    let uid = g.world.armies[0].uid;
    let barred = HeroCells { cells: [Some((30, 10)), None], at: (30, 10) };
    let step = |g: &mut Game, walked: i32, no_path: bool| {
        let m = &mut g.world.armies[0].mind;
        (m.scripted, m.idle, m.countdown, m.walked, m.no_path) = (true, 3, 4, walked, no_path);
        g.world.armies[0].path.clear();
        g.world.armies[0].budget = 25.0;
        g.ai_walk(uid, &barred);
        let m = &g.world.armies[0].mind;
        (m.idle, m.walked)
    };
    assert_eq!(step(&mut g, 0, false), (0, 0), "a one-cell path at index 0: a step");
    assert_eq!(step(&mut g, 2, false), (4, 0), "the index moved on: past the end");
    assert_eq!(step(&mut g, 0, true), (4, 0), "no path at all (map load, respawn)");
    // A plan that reads a path puts the index back to 0; one with no seed leaves it.
    let mut g = start(&s);
    assert!(g.world.armies[0].mind.no_path, "a fresh record has no path");
    let m = &mut g.world.armies[0].mind;
    (m.walked, m.wander) = (3, [(0, 0); 4]);
    g.world.armies[0].ai.no_buildings = true;
    g.world.armies[0].ai.no_random = true;
    plan(&mut g, 0);
    assert_eq!((g.world.armies[0].mind.walked, g.world.armies[0].mind.no_path), (3, false), "no seed");
    g.world.armies[0].ai.no_random = false;
    g.world.armies[0].mind.wander = [(40, 10), (0, 0), (0, 0), (0, 0)];
    plan(&mut g, 0);
    assert_eq!(g.world.armies[0].mind.walked, 0, "a path read");
}

#[test]
fn a_step_costs_the_cell_left_and_the_countdown_runs_out() {
    // AIGetPathDistance 5: after a plan, five steps then a plan again (nobody near).
    let mut s = map();
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    g.world.armies[0].mind.countdown = 5;
    g.world.armies[0].path = (31..=40).map(|x| (x, 10)).collect();
    let plans = g.ai_stats.paths;
    g.world.armies[0].budget = 4.0 * 25.0;
    let hero = HeroCells { cells: [Some(g.tile()), None], at: g.tile() };
    let uid = g.world.armies[0].uid;
    g.ai_walk(uid, &hero);
    assert_eq!((g.world.armies[0].tile(&g.world.map), g.world.armies[0].mind.countdown, g.ai_stats.paths), ((34, 10), 1, plans));
    g.world.armies[0].budget = 25.0;
    g.ai_walk(uid, &hero);
    assert_eq!(g.ai_stats.paths, plans + 1, "the countdown ran out: a plan");
}

#[test]
fn a_neighbour_near_makes_it_plan_at_every_step_and_counts_talk() {
    // Any party within AIGetPathDistance (5) re-plans at every arrival; every other party
    // at a distance gets +1 on the talk counter, a friendly one relation + 1 more.
    let mut s = map();
    let friend = army(2, (34, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    a.no_random_targets = 1;
    s.armies = vec![a, friend];
    let mut g = start(&s);
    g.world.armies[1].patrols = true;
    g.world.armies[1].patrol_radius = 0;
    g.world.armies[0].mind.scripted = true;
    g.world.armies[0].path = vec![(31, 10)];
    g.world.armies[0].budget = 25.0;
    let hero = HeroCells { cells: [Some(g.tile()), None], at: g.tile() };
    let uid = g.world.armies[0].uid;
    g.ai_walk(uid, &hero);
    // Relation to its own faction: (3 + 3) div 2 = 3; +1 for the distance, +4 at contact
    // range or not.
    let a = &g.world.armies[0];
    assert_eq!(a.mind.talk.get(&2), Some(&(1 + 3 + 1)));
    assert_eq!(a.talk, 1, "the hero: hostile, only the 1");
}

/// Army `uid`'s arrival rules, as after a step.
fn arrive(g: &mut Game, uid: u32) -> Option<Contact> {
    g.ai_arrive(uid)
}

#[test]
fn hostile_neighbours_fight_and_friendly_ones_greet() {
    let mut s = map();
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(6, 0, 3)]), army(2, (31, 11), 2, ALLY, 0, &[troop(4, 0, 1)]), army(3, (29, 9), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    // The map load scored the pair; with no positive score, no fight.
    assert!(g.world.armies[0].mind.scores[&2] > 0);
    g.world.armies[0].mind.scores.insert(2, 0);
    arrive(&mut g, 1);
    assert_eq!(g.world.armies.len(), 3);
    // The two of faction 4 greet: both counters −500, both re-plan.
    assert_eq!((g.world.armies[0].mind.talk.get(&3), g.world.armies[2].mind.talk.get(&1)), (Some(&TALKED), Some(&TALKED)));
    assert_eq!((g.world.armies[0].mind.countdown, g.world.armies[2].mind.countdown), (0, 0));
    // A positive score: army 1 attacks army 2 and wins.
    g.world.armies[0].mind.scores.insert(2, 5);
    arrive(&mut g, 1);
    assert!(g.world.armies.iter().all(|a| a.id != 2), "beaten and off the map");
    assert!(g.ai_beaten.contains(&2));
}

#[test]
fn a_hostile_neighbour_in_someone_elses_building_triples_the_scores() {
    let mut s = map();
    let mut tavern = building(BuildingType::Tavern, 31, 10, (1, 1));
    tavern.faction = 3;
    s.buildings = vec![tavern];
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(6, 0, 3)]), army(2, (31, 10), 2, ALLY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    g.world.armies[0].mind.scores.insert(2, 4000);
    g.world.armies[1].mind.scores.insert(1, -7);
    arrive(&mut g, 1);
    assert_eq!(g.world.armies.len(), 2, "no fight in a building of a third party");
    assert_eq!((g.world.armies[0].mind.scores[&2], g.world.armies[1].mind.scores[&1]), (10_000, -7), "×3 when positive, capped at 10000");
}

#[test]
fn ai_battle_loot_is_asymmetric() {
    // VictoryGoldDiv 2, MinVictoryGold 25.
    let mut s = map();
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(6, 2, 4)]), army(2, (31, 10), 2, ALLY, 0, &[troop(4, 0, 2)])];
    let mut g = start(&s);
    let wages = g.army_totals(1).wages;
    assert!(wages > 0);
    // The attacker wins: the loser's wage bill and gold div 2, with no minimum rule.
    (g.world.armies[0].gold, g.world.armies[1].gold) = (0, 20);
    assert!(g.ai_battle(0, Defender::Army(1)));
    assert_eq!(g.world.armies[0].gold, wages + 10);
    assert_eq!(g.world.respawns.len(), 0, "no home: no respawn");
    // The defender wins: the attacker's wage bill (it is feudal) and all its gold below 25.
    let mut g = start(&s);
    let wages = g.army_totals(1).wages;
    (g.world.armies[0].gold, g.world.armies[1].gold) = (0, 20);
    assert!(!g.ai_battle(1, Defender::Army(0)));
    assert_eq!(g.world.armies[0].gold, wages + 20);
    let w = &g.world.armies[0];
    assert!(w.troops.iter().all(|t| t.alive()) && w.troops.iter().any(|t| t.hurt > 0 || t.xp > 0), "{:?}", w.troops);
}

#[test]
fn a_surviving_sides_dead_stay_in_its_record_and_its_leader_lives() {
    let mut s = map();
    let mut weak = army(2, (31, 10), 2, ALLY, 0, &[troop(4, 0, 3)]);
    weak.leader_unit = 4;
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(5, 0, 1)]), weak];
    let mut g = start(&s);
    // Hurt them so a single shooter can kill some before the turn limit.
    for t in g.world.armies[1].troops.iter_mut() {
        t.hurt = 45;
    }
    let won = g.ai_battle(0, Defender::Army(1));
    assert!(!won, "a shooter alone cannot kill them all in time");
    let b = g.world.armies.iter().find(|a| a.id == 2).unwrap();
    assert!(b.troops[0].alive(), "the leader of a side that lives is at 1 HP or more");
    assert_eq!(b.troops.len(), 4, "the dead stay in the record");
    assert!(b.troops.iter().any(|t| t.died_at.is_some()), "{:?}", b.troops);
}

#[test]
fn a_hostile_village_is_taken_and_a_town_only_at_minus_3() {
    let mut s = map();
    let mut village = building(BuildingType::Village, 30, 10, (1, 1));
    village.faction = 2;
    village.relations = [1, 3, 1, -2];
    s.buildings = vec![village];
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(6, 0, 1)])];
    let mut g = start(&s);
    g.world.locations[0].tribute_gold = 40;
    g.world.locations[0].tribute_mana = 7;
    let gold = g.world.armies[0].gold;
    arrive(&mut g, 1);
    let l = &g.world.locations[0];
    assert_eq!((l.owner, l.faction, l.relations), (Owner::Army(1), 4, ENEMY), "its owner, faction and attitudes copied");
    assert_eq!(g.world.armies[0].home, Some(0), "its home, having none");
    // A feudal army takes the whole gold stock; the mana is thrown away.
    assert_eq!((g.world.armies[0].gold - gold, l.tribute_gold, l.tribute_mana), (40, 0, 0));
    assert_eq!(g.world.armies[0].mind.buildings[0], 0, "the visit zeroes its score");
    // A town: assaulted only at attitude −3.
    for (att, taken) in [(-2, false), (-3, true)] {
        let mut s = map();
        let mut town = building(BuildingType::Town, 30, 10, (1, 1));
        town.faction = 2;
        s.buildings = vec![town];
        let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(6, 0, 1)]);
        a.relations[1] = att;
        s.armies = vec![a];
        let mut g = start(&s);
        arrive(&mut g, 1);
        assert_eq!(g.world.locations[0].owner == Owner::Army(1), taken, "attitude {att}");
    }
}

#[test]
fn altars_and_ruins_won_go_neutral_and_a_tavern_is_fought_though_never_scored() {
    for (kind, owner) in [(BuildingType::Altar, Owner::Neutral), (BuildingType::Tavern, Owner::Army(9))] {
        let mut s = map();
        let mut b = building(kind, 30, 10, (1, 1));
        b.faction = 2;
        b.owner_army = 9;
        s.buildings = vec![b];
        s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(6, 0, 1)])];
        let mut g = start(&s);
        assert_eq!(g.building_score(0, 0), 0, "{kind:?}: no attack part");
        arrive(&mut g, 1);
        let l = &g.world.locations[0];
        assert_eq!(l.owner, owner, "{kind:?}");
        if kind == BuildingType::Altar {
            assert_eq!((l.faction, l.relations), (3, [0; 4]));
        }
        assert_eq!(g.world.armies[0].mind.standing, Some(0), "won: it stands in it");
    }
}

#[test]
fn a_rogue_takes_no_village_gold() {
    let mut s = map();
    let mut village = building(BuildingType::Village, 30, 10, (1, 1));
    village.faction = 4;
    s.buildings = vec![village];
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 1, &[troop(6, 0, 1)])];
    let mut g = start(&s);
    g.world.locations[0].tribute_gold = 40;
    arrive(&mut g, 1);
    assert_eq!(g.world.locations[0].tribute_gold, 40);
}

/// A friendly market with `goods` on sale at (30, 10) and army 1 (two unit 4s) in it.
fn at_market(goods: &[u32]) -> Game {
    let mut s = map();
    let mut m = building(BuildingType::Market, 30, 10, (1, 1));
    m.faction = 4;
    m.relations = [0, 0, 0, 1];
    s.buildings = vec![m];
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 2)])];
    let mut g = start(&s);
    g.world.locations[0].shop = Some(crate::rules::world::Shop { fixed: Vec::new(), random: goods.len(), price: (0, 0), stock: goods.iter().map(|&i| ItemId(i)).collect() });
    g
}

#[test]
fn shopping_sells_the_pack_at_half_then_buys_by_tactical_gain() {
    let mut g = at_market(&[11, 13, 7]);
    let a = &mut g.world.armies[0];
    a.gold = 10_000;
    a.items = vec![ItemId(9)];
    let gold = a.gold;
    arrive(&mut g, 1);
    let a = &g.world.armies[0];
    // The potion sold for RelationPrice(100) div 2 = 50 at attitude 1; the +10 ring and the
    // +10 amulet bought (100 each), the plain ring (no gain) left.
    assert!(a.items.is_empty());
    let worn: Vec<ItemId> = a.troops.iter().flat_map(|t| t.worn.iter().flatten().copied()).collect();
    assert_eq!(worn, [ItemId(11), ItemId(13)], "both on the first unit, the first of equals");
    assert_eq!(a.gold, gold + 50 - 200);
    assert_eq!(g.world.locations[0].shop.as_ref().unwrap().stock, [ItemId(7)]);
}

#[test]
fn a_good_bought_for_a_unit_that_can_no_longer_wear_it_is_paid_and_lost() {
    // The values are not recomputed between purchases (0x4a548c): the +10 and the +6 ring
    // both go to the first unit; the second cannot be worn there, but is paid for.
    let mut g = at_market(&[11, 12]);
    g.world.armies[0].gold = 10_000;
    let gold = g.world.armies[0].gold;
    arrive(&mut g, 1);
    let a = &g.world.armies[0];
    let worn: Vec<ItemId> = a.troops.iter().flat_map(|t| t.worn.iter().flatten().copied()).collect();
    assert_eq!(worn, [ItemId(11)]);
    assert_eq!(a.gold, gold - 200, "both paid");
    assert!(g.world.locations[0].shop.as_ref().unwrap().stock.is_empty(), "both gone");
}

#[test]
fn no_shopping_when_the_cheapest_good_is_beyond_its_spare_gold() {
    let mut g = at_market(&[11]);
    g.world.armies[0].gold = 0;
    arrive(&mut g, 1);
    assert_eq!(g.world.locations[0].shop.as_ref().unwrap().stock, [ItemId(11)]);
}

#[test]
fn a_good_of_negative_price_is_bought_at_its_absolute_price() {
    // The purchase table holds |RelationPrice(price)| of the raw price (0x4a548c): a ring
    // priced −100 costs 100 at attitude 1.
    let mut c = content();
    c.items.iter_mut().find(|d| d.id == 11).unwrap().cost = -100;
    let mut g = at_market(&[11]);
    g.content = Arc::new(c);
    g.world.armies[0].gold = 10_000;
    arrive(&mut g, 1);
    let a = &g.world.armies[0];
    assert_eq!(a.troops[0].worn.iter().flatten().copied().collect::<Vec<_>>(), [ItemId(11)]);
    assert_eq!(a.gold, 10_000 - 100);
}

/// A friendly church with services at (30, 10), barracks of unit 4, and army 1 in it.
fn at_church(troops: &[DtTroop]) -> Game {
    let mut s = map();
    let mut c = building(BuildingType::Church, 30, 10, (1, 1));
    c.faction = 4;
    c.relations = [0, 0, 0, 1];
    c.has_barracks = 1;
    c.barracks[0] = RecruitSlot { unit: 4, start_count: 0, max_count: 5 };
    s.buildings = vec![c];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, troops);
    a.leader_unit = 6;
    s.armies = vec![a];
    start(&s)
}

#[test]
fn healing_is_priced_by_the_current_hp_and_keeps_it_busy() {
    let mut g = at_church(&[troop(4, 0, 1)]);
    let now = g.clock.total_minutes();
    let a = &mut g.world.armies[0];
    a.gold = 1000;
    // The leader at 30 of 120: Round(200 / (100/50) × 30/120) = 25, attitude 1: 25.
    a.troops[0].hurt = 90;
    arrive(&mut g, 1);
    let a = &g.world.armies[0];
    assert_eq!((a.gold, a.troops[0].hurt), (1000 - 25, 0));
    assert_eq!(a.mind.busy_until, now + 60.0, "HealingTime");
    // Busy: it banks nothing, takes no step.
    let pos = a.pos;
    g.wait(1);
    assert!(g.world.armies[0].budget == 0.0 && g.world.armies[0].pos == pos);
}

#[test]
fn the_dead_are_raised_in_a_church_the_dearest_first() {
    let mut g = at_church(&[troop(4, 0, 1)]);
    let a = &mut g.world.armies[0];
    a.gold = 1000;
    a.troops[1].died_at = Some(g.clock.total_minutes() as u64);
    arrive(&mut g, 1);
    // Round(50 × 300/100) = 150.
    let a = &g.world.armies[0];
    assert_eq!((a.gold, a.troops[1].alive()), (1000 - 150, true));
}

#[test]
fn the_roles_are_ordered_by_their_sums_as_the_original() {
    let c = content();
    let t = |u: u32| Troop::new(UnitId(u), 1, slot(0));
    // Warriors only: w > s = m = 0: mages, then shooters (w < s fails), then warriors.
    assert_eq!(Game::role_order(&c, &[t(4)], 0), [0x11, 7, 4]);
    // A shooter only: w < s but not w < m, nor s < m: mages, then warriors (w < s).
    assert_eq!(Game::role_order(&c, &[t(5)], 0), [0x11, 4, 7]);
    assert_eq!([attack_kind(&c, UnitId(4)), attack_kind(&c, UnitId(5))], [4, 7]);
}

#[test]
fn hiring_by_role_and_nature_as_a_recruit_at_home_a_mercenary_abroad() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 30, 10, (1, 1));
    castle.faction = 4;
    castle.owner_army = 1;
    castle.has_barracks = 1;
    castle.barracks[0] = RecruitSlot { unit: 4, start_count: 2, max_count: 5 };
    castle.barracks[1] = RecruitSlot { unit: 5, start_count: 1, max_count: 5 };
    castle.barracks[2] = RecruitSlot { unit: 7, start_count: 5, max_count: 5 };
    s.buildings = vec![castle];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[]);
    a.leader_unit = 6;
    s.armies = vec![a];
    let mut g = start(&s);
    g.world.armies[0].gold = 10_000;
    arrive(&mut g, 1);
    let units: Vec<u32> = g.world.armies[0].troops.iter().map(|t| t.unit.0).collect();
    // The leader is a warrior: mages, then shooters, then warriors; no rogue (another
    // Nature).
    assert_eq!(units, [6, 5, 4, 4]);
    assert!(g.world.armies[0].troops[1..].iter().all(|t| t.kind == WageKind::Recruit), "in its own building");
    // Abroad they are mercenaries.
    let mut g = start(&s);
    g.world.locations[0].owner = Owner::Neutral;
    g.world.armies[0].gold = 10_000;
    arrive(&mut g, 1);
    assert!(g.world.armies[0].troops[1..].iter().all(|t| t.kind == WageKind::Mercenary));
}

#[test]
fn the_third_role_caps_hiring_at_8_only_before_a_pass() {
    // Shooters (first), then warriors, then mages: the warriors come in the third role
    // here, so the cap falls to 8; the pass that sets it still hires.
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 30, 10, (1, 1));
    castle.faction = 4;
    castle.owner_army = 1;
    castle.has_barracks = 1;
    castle.barracks[0] = RecruitSlot { unit: 4, start_count: 20, max_count: 20 };
    s.buildings = vec![castle];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 8)]);
    a.leader_unit = 6;
    s.armies = vec![a];
    let mut g = start(&s);
    g.world.armies[0].gold = 100_000;
    arrive(&mut g, 1);
    assert_eq!(g.world.armies[0].troops.len(), 10, "9 units, then one more in the pass that set the cap");
}

#[test]
fn a_hire_from_the_last_barracks_slot_of_the_second_role_sets_the_cap_of_8() {
    // The scan reads the six slots, empty ones included (0x4a548c): a warrior hired from the
    // sixth slot while warriors are the second role moves the scan on into the third role,
    // which lowers the cap to 8 before the next pass. From the first slot it does not.
    let hired = |slot: usize| {
        let mut s = map();
        let mut castle = building(BuildingType::Castle, 30, 10, (1, 1));
        castle.faction = 4;
        castle.owner_army = 1;
        castle.has_barracks = 1;
        castle.barracks[slot] = RecruitSlot { unit: 4, start_count: 20, max_count: 20 };
        s.buildings = vec![castle];
        // Shooters only: mages first (none), warriors second, shooters third.
        let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(5, 0, 7)]);
        a.leader_unit = 5;
        s.armies = vec![a];
        let mut g = start(&s);
        g.world.armies[0].gold = 100_000;
        arrive(&mut g, 1);
        g.world.armies[0].troops.len()
    };
    assert_eq!(hired(5), 9, "one hire, then the cap of 8 stops it");
    assert!(hired(0) > 9);
}

/// Army 1 (leader unit 6 and two unit 4s, garrison level `level`) in its own castle at
/// (30, 10) with an empty garrison.
fn at_own_castle(level: u8, gold: i32) -> Game {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 30, 10, (1, 1));
    castle.faction = 4;
    castle.owner_army = 1;
    s.buildings = vec![castle];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 2)]);
    a.leader_unit = 6;
    a.garrison_strength = level;
    s.armies = vec![a];
    let mut g = start_with(&s, with_priorities());
    g.world.armies[0].gold = gold;
    g
}

#[test]
fn the_reshuffle_deals_the_units_by_the_quota_table() {
    // Level 20, defence 0: q = 20 < 50, the garrison row is 60% of the pool's sums, the
    // army's 0. Mages and shooters (none) are charged 100000 each first; then the army's
    // warrior cell (0) takes a warrior, and, with spare gold, the other.
    let mut g = at_own_castle(20, 10_000);
    arrive(&mut g, 1);
    assert_eq!((g.world.armies[0].troops.len(), g.world.locations[0].garrison.len()), (3, 0));
    // Without spare gold the army's row is closed after its first unit: the other warrior
    // goes to the garrison.
    let mut g = at_own_castle(20, 0);
    arrive(&mut g, 1);
    assert_eq!((g.world.armies[0].troops.len(), g.world.locations[0].garrison.len()), (2, 1));
    // Without a garrison level: no reshuffle.
    let mut g = at_own_castle(0, 0);
    arrive(&mut g, 1);
    assert_eq!((g.world.armies[0].troops.len(), g.world.locations[0].garrison.len()), (3, 0));
}

#[test]
fn garrison_buying_deducts_the_price_whatever_the_gold() {
    // While its spare gold is above a third of the gold it started with: the whole stock
    // (5), each at the price at +3 (its own building).
    let mut g = at_own_castle(20, 10_000);
    g.world.locations[0].recruits = vec![crate::rules::world::Recruit::new(UnitId(4), 5, 5)];
    assert!(g.ai_buy_garrison(0, 0));
    assert_eq!((g.world.locations[0].garrison.len(), g.world.armies[0].gold), (5, 10_000 - 5 * 38));
    // Warriors come third for an empty garrison (mages, shooters, warriors), so the cap is 8.
    let mut g = at_own_castle(20, 3_400);
    g.world.locations[0].recruits = vec![crate::rules::world::Recruit::new(UnitId(4), 50, 50)];
    g.ai_buy_garrison(0, 0);
    assert_eq!(g.world.locations[0].garrison.len(), 8);
    let mut g = at_own_castle(20, 900);
    g.world.locations[0].recruits = vec![crate::rules::world::Recruit::new(UnitId(4), 50, 50)];
    g.ai_buy_garrison(0, 0);
    // 900 − 520 + 200 = 580 against a third of 900, 300: tested before each buy, the
    // eighth takes it from 314 to 276.
    let spare = g.spare_gold(0);
    assert!(spare <= 300 && spare + 38 > 300, "{spare}");
}

#[test]
fn a_beaten_army_respawns_at_its_home_whole_when_the_ai_beat_it() {
    let mut s = map();
    let mut fort = building(BuildingType::Fort, 40, 10, (1, 1));
    fort.faction = 4;
    fort.owner_army = 1;
    s.buildings = vec![fort];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 2)]);
    a.leader_unit = 6;
    a.home_building = 1;
    a.respawn_days = 2;
    a.unknown_80 = 3;
    s.armies = vec![a];
    let mut g = start(&s);
    g.world.armies[0].gold = 7;
    g.world.armies[0].troops[1].died_at = Some(0);
    g.army_beaten(0, Beaten::ByAi);
    assert!(g.world.armies.is_empty());
    // Two days later, not before (strictly after the delay).
    let due = g.world.respawns[0].due;
    assert_eq!(due, g.clock.total_minutes() + 2.0 * MINUTES_PER_DAY as f64);
    g.ai_respawns(due);
    assert!(g.world.armies.is_empty());
    g.ai_respawns(due + 1.0);
    let a = &g.world.armies[0];
    assert_eq!(a.troops.len(), 3, "the whole army, its dead raised");
    assert!(a.troops.iter().all(|t| t.alive() && t.hurt == 0));
    assert_eq!(a.tile(&g.world.map), (40, 10), "at its home's centre");
    assert_eq!(a.gold - 7, 2 * 30, "the delay's days of income");
    assert_eq!((a.mind.just_respawned, a.mind.wander[0]), (true, (30, 10)), "its first wander point its post");
    // Beaten by the player, only its leader comes back, unless it respawns whole.
    let mut g = start(&s);
    g.army_beaten(0, Beaten::ByPlayer);
    assert_eq!(g.world.respawns[0].army.troops.len(), 1);
    let mut g = start(&s);
    g.world.armies[0].ai.respawn_all = true;
    g.army_beaten(0, Beaten::ByPlayer);
    assert_eq!(g.world.respawns[0].army.troops.len(), 3);
}

#[test]
fn where_a_beaten_army_comes_back() {
    // A feudal army that lost its home: its first town, castle or fort, in that order;
    // none: never.
    let mut s = map();
    let mut home = building(BuildingType::Village, 40, 10, (1, 1));
    home.faction = 3;
    let mut fort = building(BuildingType::Fort, 20, 5, (1, 1));
    fort.faction = 4;
    fort.owner_army = 1;
    let mut castle = building(BuildingType::Castle, 50, 5, (1, 1));
    castle.faction = 4;
    castle.owner_army = 1;
    s.buildings = vec![home, fort, castle];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    a.home_building = 1;
    a.respawn_days = 1;
    s.armies = vec![a.clone()];
    let back = |g: &mut Game| {
        let due = g.world.respawns[0].due;
        g.ai_respawns(due + 1.0);
    };
    let mut g = start(&s);
    g.army_beaten(0, Beaten::ByAi);
    back(&mut g);
    assert_eq!(g.world.armies[0].tile(&g.world.map), (50, 5), "the castle before the fort");
    let mut g = start(&s);
    g.world.locations[1].owner = Owner::Neutral;
    g.world.locations[2].owner = Owner::Neutral;
    g.army_beaten(0, Beaten::ByAi);
    back(&mut g);
    assert!(g.world.armies.is_empty() && g.world.respawns.is_empty(), "owning none, never");
    // A rogue takes over its home when it is a village, shipyard, altar or ruins, from
    // anyone, the player included.
    for (kind, taken) in [(BuildingType::Ruins, true), (BuildingType::Village, true), (BuildingType::DungeonEntrance, false)] {
        let mut s = map();
        let mut home = building(kind, 40, 10, (1, 1));
        home.faction = 1;
        s.buildings = vec![home];
        let mut a = army(1, (30, 10), 4, ENEMY, 1, &[troop(4, 0, 1)]);
        a.home_building = 1;
        a.respawn_days = 1;
        s.armies = vec![a];
        let mut g = start(&s);
        assert!(g.world.locations[0].owned());
        g.army_beaten(0, Beaten::ByAi);
        back(&mut g);
        assert_eq!(g.world.locations[0].owner == Owner::Army(1), taken, "{kind:?}");
        assert_eq!(g.world.armies[0].tile(&g.world.map), (40, 10));
    }
}

#[test]
fn an_armys_noon_is_lazy_and_counts_its_castles_stock() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 50, 5, (1, 1));
    castle.faction = 4;
    castle.owner_army = 1;
    castle.gold_per_day = 40;
    s.buildings = vec![castle];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 2)]);
    a.leader_unit = 6;
    a.unknown_80 = 2;
    s.armies = vec![a];
    let mut g = start(&s);
    // The map starts at 09:00: its first noon is today's 12:00, and the map load counted
    // its castle's income into today's (20 + 40).
    let day = MINUTES_PER_DAY as f64;
    let noon = (g.clock.total_minutes() / day).floor() * day + day / 2.0;
    assert_eq!((g.world.armies[0].mind.next_noon, g.world.armies[0].mind.income), (noon, 20 + 40));
    g.world.locations[0].tribute_gold = 33;
    g.world.armies[0].gold = 100;
    let wages = g.army_totals(0).wages;
    // Not before an arrival after 12:00.
    g.ai_noon(0, noon + 1.0);
    let a = &g.world.armies[0];
    assert_eq!(a.gold, 100 + 20 + 33 - wages, "its income and the castle's stock; wages paid");
    assert_eq!(a.mind.income, 20 + 40, "today's income keeps the castle's income");
    assert_eq!(a.mind.next_noon, noon + day, "tomorrow's");
    assert_eq!(g.world.locations[0].tribute_gold, 0);
    // A rogue pays no wages.
    g.world.armies[0].ai.style = Style::Rogue;
    let gold = g.world.armies[0].gold;
    g.ai_noon(0, noon + day + 1.0);
    assert_eq!(g.world.armies[0].gold, gold + 20);
}

#[test]
fn a_feudal_noon_and_the_heros_mark_their_pairs_dirty() {
    // The wage payment ends with 0x4a26e8: a feudal army's pairs, both ways, are rescored at
    // the next plan; a rogue's noon pays nothing and marks nothing. The hero's noon too.
    let mut s = map();
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]), army(2, (32, 10), 2, ALLY, 1, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    let now = g.clock.total_minutes();
    let clean = |g: &Game, i: usize, key: u32| g.world.armies[i].mind.clean.contains(&key);
    assert!(clean(&g, 0, 2) && clean(&g, 1, 1), "the map load scored the pair");
    g.ai_noon(1, now);
    assert!(clean(&g, 0, 2) && clean(&g, 1, 1), "a rogue's noon");
    g.ai_noon(0, now);
    assert!(!clean(&g, 0, 2) && !clean(&g, 1, 1));
    g.world.armies[1].mind.clean.insert(HERO);
    g.pay_noon();
    assert!(!clean(&g, 1, HERO));
}

#[test]
fn short_gold_at_noon_leaves_the_cheapest_unpaid_and_old_unpaid_ones_desert() {
    let mut s = map();
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1), troop(6, 0, 1)]);
    a.leader_unit = 6;
    s.armies = vec![a];
    let mut g = start(&s);
    let now = g.clock.total_minutes();
    g.world.armies[0].gold = 10;
    // Wages 6 (unit 4) and 50 (unit 6): 10 − 56 < 0, the cheapest refunded first, then the
    // other; both unpaid, the gold set to 0.
    g.ai_noon(0, now);
    let a = &g.world.armies[0];
    assert_eq!(a.gold, 0);
    assert_eq!(a.troops.iter().map(|t| t.unpaid).collect::<Vec<_>>(), [false, true, true]);
    // Unpaid units do not fight when it attacks.
    assert_eq!(g.army_side(0, true).0.units.len(), 1);
    // A week on, still short: they leave.
    g.world.armies[0].gold = 0;
    g.ai_noon(0, now + 7.0 * MINUTES_PER_DAY as f64 + 1.0);
    assert_eq!(g.world.armies[0].troops.len(), 1, "only the leader");
}

#[test]
fn midnight_averages_the_village_gold() {
    let mut s = map();
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    let m = &mut g.world.armies[0].mind;
    (m.village_avg, m.village_today) = (50, 31);
    g.ai_midnight();
    let m = &g.world.armies[0].mind;
    assert_eq!((m.village_avg, m.village_today), ((50 + 31) / 2, 0));
}

/// Content whose unit 9 can become 10 or 11 (two options: slots 1 and 3), militia 4 guard 5
/// or squire 6, unit 8 the same, all from level 1 (0-based).
fn tree_content() -> Content {
    use crate::rules::content::Upgrade;
    let up = |target: u32, slot: u8| Upgrade { target_name: String::new(), target: Some(target), level: 1, slot };
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
    units[1].upgrades = vec![up(5, 1), up(6, 3)];
    units[4].upgrades = vec![up(5, 1), up(6, 3)];
    units[5].upgrades = vec![up(10, 1), up(11, 3)];
    ck::content(units, vec![ck::item(7, ArtefactType::Ring)])
}

#[test]
fn ai_units_take_the_upgrade_tree_with_the_originals_rolls() {
    let c = tree_content();
    let mut pool = Vec::new();
    // Below the level: no promotion, but the roll is drawn (a militia's single Rand(3)).
    let mut rng = Rng::new(7);
    let mut t = Troop::new(UnitId(4), 1, slot(0));
    ai_gain_xp(&c, &mut rng, &mut t, 50, 100, &mut pool);
    assert_eq!((t.unit, t.level, t.xp), (UnitId(4), 1, 50));
    let mut once = Rng::new(7);
    once.random(3);
    assert_eq!(rng.state(), once.state(), "one Rand(3)");
    // A level reached: the new class at level 1 (0-based 0), no XP, its items to the pool.
    let mut picks = std::collections::BTreeMap::new();
    for _ in 0..60 {
        let mut t = Troop::new(UnitId(9), 1, slot(0));
        t.worn[0] = Some(ItemId(7));
        ai_gain_xp(&c, &mut rng, &mut t, 70, 100, &mut pool);
        assert_eq!((t.level, t.xp, t.worn[0]), (1, 0, None));
        *picks.entry(t.unit.0).or_insert(0) += 1;
    }
    assert_eq!(picks.keys().copied().collect::<Vec<_>>(), vec![10, 11], "both branches");
    assert_eq!(pool.len(), 60);
    // Militia: slot 1 when Rand(3) = 0, else slot 3; Infantry the other way round.
    let mut r = Rng::new(1);
    let first = r.random(3);
    let mut rng = Rng::new(1);
    let mut m = Troop::new(UnitId(4), 1, slot(0));
    ai_gain_xp(&c, &mut rng, &mut m, 60, 100, &mut pool);
    assert_eq!(m.unit, UnitId(if first == 0 { 5 } else { 6 }));
    let mut rng = Rng::new(1);
    let mut q = Troop::new(UnitId(8), 1, slot(0));
    ai_gain_xp(&c, &mut rng, &mut q, 60, 100, &mut pool);
    assert_eq!(q.unit, UnitId(if first == 0 { 6 } else { 5 }));
}

#[test]
fn hired_units_get_their_xp_level_by_level() {
    // X = (P − its tactical cost div 2 when P ≥ 1) + bonus; fed Rand(X) + X div 2.
    let mut s = map();
    let mut c1 = building(BuildingType::Castle, 30, 10, (1, 1));
    c1.faction = 4;
    c1.owner_army = 1;
    c1.has_barracks = 1;
    c1.barracks[0] = RecruitSlot { unit: 4, start_count: 1, max_count: 5 };
    s.buildings = vec![c1];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[]);
    a.leader_unit = 6;
    a.hire_bonus_exp = 100;
    s.armies = vec![a];
    let mut g = start(&s);
    g.world.armies[0].gold = 10_000;
    let mut rng = g.rng.clone();
    let x = 100;
    let xp = rng.random(x) + x / 2;
    arrive(&mut g, 1);
    let t = g.world.armies[0].troops[1];
    // 60 for the first level.
    let (level, rest) = if xp >= 60 { (2, xp - 60) } else { (1, xp) };
    assert_eq!((t.level, t.xp), (level, rest));
}

#[test]
fn the_plan_goes_for_the_best_target_through_the_flood() {
    // Two villages with gold, the nearer one with less: the flood weighs score and way.
    let mut s = map();
    s.buildings = vec![building(BuildingType::Village, 36, 10, (1, 1)), building(BuildingType::Village, 20, 10, (1, 1))];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    a.no_random_targets = 1;
    s.armies = vec![a];
    let mut g = start_with(&s, with_priorities());
    g.world.armies[0].gold = 0;
    g.world.armies[0].mind.scores.insert(HERO, 0);
    g.world.locations[0].tribute_gold = 10;
    g.world.locations[1].tribute_gold = 90;
    g.rescore_buildings(0);
    let scores = g.world.armies[0].mind.buildings.clone();
    // Round(0.9·100) + 10 = 100 against Round(0.1·100) + 10 = 20.
    assert_eq!(scores, [100, 20]);
    plan(&mut g, 0);
    let a = &g.world.armies[0];
    // 10 cells of grass (5 × 2 each) = 100 more for the far one: 20 + 1 + 100 < 100 + 1 + 60.
    assert_eq!(a.path.last(), Some(&(20, 10)), "{:?}", a.path);
    assert_eq!(a.mind.countdown, 5);
}

#[test]
fn a_danger_pushes_a_repulsion_cone_onto_the_way() {
    // A hostile army it would lose to, on the straight way to its target: the way bends.
    let mut s = map();
    s.buildings = vec![building(BuildingType::Village, 40, 10, (1, 1))];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    a.no_random_targets = 1;
    let mut foe = army(2, (35, 10), 2, ALLY, 0, &[troop(6, 0, 6)]);
    foe.patrols = 1;
    s.armies = vec![a, foe];
    let mut g = start_with(&s, with_priorities());
    g.world.armies[0].gold = 0;
    g.world.armies[0].mind.scores.insert(HERO, 0);
    g.world.locations[0].tribute_gold = 10;
    g.rescore_buildings(0);
    plan(&mut g, 0);
    // A stationary guard: not scored at map load, scored now (within AIDistance).
    let danger = g.world.armies[0].mind.scores[&2];
    assert!(danger < -5, "{danger}");
    let path = g.world.armies[0].path.clone();
    assert_eq!(path.last(), Some(&(40, 10)));
    assert!(path.iter().all(|t| t.1 != 10 || (t.0 - 35).abs() > 2), "around it: {path:?}");
}

#[test]
fn healing_lowers_the_stored_scores_of_the_service_buildings() {
    let mut s = map();
    let mut church = building(BuildingType::Church, 40, 10, (1, 1));
    church.faction = 4;
    church.has_barracks = 1;
    s.buildings = vec![church, building(BuildingType::Village, 20, 10, (1, 1))];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(6, 0, 1)]);
    a.no_random_targets = 1;
    s.armies = vec![a];
    let mut c = with_priorities();
    c.options.ai_targets.min_healing = Some([20; 5]);
    let mut g = start_with(&s, c);
    g.world.armies[0].gold = 10_000;
    g.world.armies[0].mind.scores.insert(HERO, 0);
    g.world.armies[0].troops[0].hurt = 60;
    // Half the HP missing: Round(0.5·(100 − 20) + 20) = 60, lowering a score above it.
    g.world.armies[0].mind.buildings = vec![500, 0];
    plan(&mut g, 0);
    assert_eq!(g.world.armies[0].mind.buildings, [60, 0], "a 0 stays 0");
    // A resurrection due: 3h at a town or church.
    g.world.armies[0].troops.push(Troop { died_at: Some(g.clock.total_minutes() as u64), ..Troop::new(UnitId(4), 1, slot(3)) });
    g.world.armies[0].mind.buildings = vec![500, 0];
    let t = g.army_totals(0);
    let h = delphi_round((1.0 - t.missing as f64 / t.max_living as f64) * 80.0 + 20.0) as i32;
    plan(&mut g, 0);
    assert_eq!(g.world.armies[0].mind.buildings[0], 3 * h);
}

#[test]
fn ships_plan_on_the_ship_map() {
    // A strait of water x 10..=19 on grass: a ship's wander points on land are no seeds.
    let mut s = scenario(30, 10);
    for y in 0..10 {
        for x in 10..20 {
            tk::set(&mut s, x, y, crate::dt::dtm::Surface::CoastalWater);
        }
    }
    s.header.heroes[0] = hero(2, 2, 0, &[]);
    let mut ship = army(1, (15, 5), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    ship.ship = crate::rules::ships::kind::PIRATE;
    let mut lander = army(2, (25, 5), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    lander.ship = crate::rules::ships::kind::PIRATE;
    s.armies = vec![ship, lander];
    let mut g = start(&s);
    assert!(g.world.armies[0].sails(), "placed on water");
    assert!(!g.world.armies[1].sails(), "byte 72 alone makes no ship: it stands on land");
    for _ in 0..20 {
        g.wait(4);
    }
    for t in std::iter::once(g.world.armies[0].tile(&g.world.map)).chain(g.world.armies[0].path.iter().copied()) {
        assert!((10..20).contains(&t.0), "the ship stays at sea: {t:?}");
    }
}

#[test]
fn only_a_bridge_keeps_an_army_on_water_from_being_a_ship() {
    // The loader (0x4b4a90) tests the bridge types only: on water in a shipyard's footprint
    // an army is a ship, on a bridge it is not.
    let mut s = scenario(30, 10);
    for y in 0..10 {
        for x in 10..20 {
            tk::set(&mut s, x, y, crate::dt::dtm::Surface::CoastalWater);
        }
    }
    s.header.heroes[0] = hero(2, 2, 0, &[]);
    s.buildings = vec![building(BuildingType::Shipyard, 12, 5, (1, 1)), building(BuildingType::WoodenBridge, 17, 5, (1, 1))];
    s.armies = vec![army(1, (12, 5), 4, ENEMY, 0, &[troop(4, 0, 1)]), army(2, (17, 5), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let g = start(&s);
    let sails = |id: u8| g.world.armies.iter().find(|a| a.id == id).map(|a| a.sails());
    assert_eq!((sails(1), sails(2)), (Some(true), Some(false)));
}

#[test]
fn the_map_loads_building_scores_before_the_income_and_average() {
    // At map load the scores see no income and no village average (0x4a1ff0 sets them after).
    let mut s = map();
    s.buildings = vec![building(BuildingType::Village, 40, 10, (1, 1))];
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)]);
    a.gold_income = 600;
    s.armies = vec![a];
    let mut g = start_with(&s, with_priorities());
    g.world.locations[0].tribute_gold = 30;
    let spare_at_load = {
        let m = &mut g.world.armies[0].mind;
        let kept = (m.income, m.village_avg);
        (m.income, m.village_avg) = (0, 0);
        let v = g.spare_gold(0);
        let m = &mut g.world.armies[0].mind;
        (m.income, m.village_avg) = kept;
        v
    };
    assert!(spare_at_load < g.spare_gold(0));
    g.ai_init(false);
    let at_load = g.world.armies[0].mind.buildings[0];
    g.rescore_buildings(0);
    assert_ne!(at_load, g.world.armies[0].mind.buildings[0], "the average of 50 counts afterwards");
}

#[test]
fn an_armys_items_go_to_the_unit_they_help_most_else_to_the_pack() {
    let mut s = map();
    let mut a = army(1, (30, 10), 4, ENEMY, 0, &[troop(5, 0, 1), troop(4, 0, 1)]);
    a.artifacts = [11, 7, 0];
    s.armies = vec![a];
    let g = start(&s);
    let a = &g.world.armies[0];
    // The +10 attack ring helps the warrior; the plain ring helps nobody: the pack.
    assert_eq!(a.troops.iter().find(|t| t.unit == UnitId(4)).unwrap().worn.iter().flatten().copied().collect::<Vec<_>>(), [ItemId(11)]);
    assert_eq!(a.items, [ItemId(7)]);
}

#[test]
fn the_hand_out_wears_the_best_gains_and_packs_the_dearest() {
    let c = content();
    let mut troops = vec![Troop::new(UnitId(4), 1, slot(0)), Troop::new(UnitId(4), 1, slot(1))];
    let mut pack = vec![ItemId(12)];
    redistribute(&c, &mut troops, &mut pack, vec![ItemId(11), ItemId(7), ItemId(9)], 0);
    // +10 to the first, +6 to the second (a gain above 5); the ring and the potion packed.
    assert_eq!((troops[0].worn[0], troops[1].worn[0]), (Some(ItemId(11)), Some(ItemId(12))));
    assert_eq!(pack.len(), 2);
}

#[test]
fn old_saves_reload_with_the_ai_set_up_again() {
    let mut s = map();
    s.armies = vec![army(1, (30, 10), 4, ENEMY, 0, &[troop(4, 0, 1)])];
    let mut g = start(&s);
    g.world.armies[0].mind = AiMind::default();
    g.world.armies[0].ai.garrison_level = -1;
    g.ai_init(false);
    let m = &g.world.armies[0].mind;
    assert!(m.next_noon > 0.0 && m.village_avg == 50, "as at map load");
}

#[test]
fn a_load_puts_the_barracks_slots_back() {
    let mut s = map();
    let mut castle = building(BuildingType::Castle, 30, 10, (1, 1));
    castle.has_barracks = 1;
    castle.barracks[2] = RecruitSlot { unit: 4, start_count: 1, max_count: 1 };
    castle.barracks[5] = RecruitSlot { unit: 5, start_count: 1, max_count: 1 };
    s.buildings = vec![castle];
    let c = content();
    let fresh = crate::rules::world::World::from_scenario(&s, &c);
    assert_eq!(fresh.locations[0].recruits.iter().map(|r| r.slot).collect::<Vec<_>>(), [2, 5]);
    let mut loaded: crate::rules::world::World = serde_json::from_str(&serde_json::to_string(&fresh).unwrap()).unwrap();
    assert_eq!(loaded.locations[0].recruits.iter().map(|r| r.slot).collect::<Vec<_>>(), [0, 0], "not saved");
    loaded.restore_statics(fresh).unwrap();
    assert_eq!(loaded.locations[0].recruits.iter().map(|r| r.slot).collect::<Vec<_>>(), [2, 5]);
}
