//! Battle rules on synthetic content, one test (at least) per row of the "Razdor now →
//! original" table in original-mechanics/battle.md.
use super::*;
use crate::rules::content::testkit::*;
use crate::rules::content::{ArtefactType, ItemId, MagicDirection, UnitDef};
use Row::*;

const fn f(col: u8) -> Slot {
    Slot::new(Front, col)
}
const fn b(col: u8) -> Slot {
    Slot::new(Back, col)
}
const fn r(col: u8) -> Slot {
    Slot::new(Reserve, col)
}

/// Test units: 10 warrior 30/5, 11 shooter 20, 12 Elemental enemy mage 40, 13 Life healer 20,
/// 14 Death mage 30, 15 Life strike mage 30, 16 undead warrior, 17 elemental warrior,
/// 18 weak warrior 10/0 with 200 HP (a punching bag), 1 = Knight hero class. All have 50 HP
/// (unless said), initiative 10 (the player's +1 makes 11) and one action.
fn units() -> Vec<UnitDef> {
    use MagicDirection::*;
    let bag = UnitDef { hits: 200, ..warrior(18, 10, 0) };
    vec![
        warrior(10, 30, 5),
        shooter(11, 20),
        mage(12, 40, MagicSchool::Elemental, ToEnemy),
        mage(13, 20, MagicSchool::Life, ToAlly),
        mage(14, 30, MagicSchool::Death, ToEnemy),
        mage(15, 30, MagicSchool::Life, ToEnemy),
        UnitDef { nature: Nature::Undead, ..warrior(16, 20, 0) },
        UnitDef { nature: Nature::Elemental, ..warrior(17, 20, 0) },
        bag,
        warrior(1, 30, 5),
    ]
}

fn content_with(extra: Vec<UnitDef>, formation: Formation) -> Arc<Content> {
    let mut us = units();
    for u in extra {
        us.retain(|x| x.id != u.id);
        us.push(u);
    }
    let mut c = content(us, vec![]);
    c.formation = formation;
    Arc::new(c)
}

/// A battle before `begin`, with `setup` applied.
fn prepared(c: &Arc<Content>, player: &[(u32, Slot)], enemies: &[(u32, Slot)], attacker: Team) -> Battle {
    let squad: Vec<Unit> = player.iter().map(|&(id, s)| Unit::new(c, UnitId(id), s)).collect();
    let p: Vec<_> = squad.iter().enumerate().collect();
    let e: Vec<Unit> = enemies.iter().map(|&(id, s)| Unit::new(c, UnitId(id), s)).collect();
    Battle::new(c.clone(), &p, &e, attacker)
}

fn battle_in(c: &Arc<Content>, player: &[(u32, Slot)], enemies: &[(u32, Slot)]) -> Battle {
    let mut bt = prepared(c, player, enemies, Team::Player);
    bt.begin();
    bt
}

fn battle(player: &[(u32, Slot)], enemies: &[(u32, Slot)]) -> Battle {
    battle_in(&content_with(vec![], Formation::WIDE), player, enemies)
}

fn with(extra: Vec<UnitDef>, player: &[(u32, Slot)], enemies: &[(u32, Slot)]) -> Battle {
    battle_in(&content_with(extra, Formation::WIDE), player, enemies)
}

/// A battle where the enemy fights in a building of `defence`.
fn in_building(extra: Vec<UnitDef>, player: &[(u32, Slot)], enemies: &[(u32, Slot)], defence: i32) -> Battle {
    let c = content_with(extra, Formation::WIDE);
    let mut bt = prepared(&c, player, enemies, Team::Player);
    bt.set_building_defence(Team::Enemy, defence);
    bt.begin();
    bt
}

/// Makes `id` the active fighter by skipping others (fails after a while).
fn turn_of(bt: &mut Battle, id: usize) {
    for _ in 0..200 {
        if bt.active() == Some(id) {
            return;
        }
        bt.skip();
    }
    panic!("fighter {id} never gets a turn");
}

fn to_round(bt: &mut Battle, round: u32) {
    while bt.round < round {
        bt.skip();
    }
}

fn bonus(id: u32, b: Bonus, base: UnitDef) -> UnitDef {
    UnitDef { id, bonus: Some(b), ..base }
}

fn acts(id: u32, n: i32, base: UnitDef) -> UnitDef {
    UnitDef { id, manevres: n, ..base }
}

/// 69: a heavy hitter, attack 100.
fn hammer() -> UnitDef {
    warrior(69, 100, 0)
}

/// 84: armour 20, 300 HP.
fn armour() -> UnitDef {
    UnitDef { hits: 300, ..warrior(84, 1, 20) }
}

// --- reach (484c4c) ----------------------------------------------------------------------

#[test]
fn warrior_hits_front_cells_c_minus_1_to_c_plus_1() {
    let bt = battle(&[(10, f(2))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(4)), (18, b(2))]);
    assert_eq!(bt.targets(0), vec![1, 2, 3]);
    assert!(bt.targets(0).iter().all(|&t| bt.options(0, t) == vec![ActionKind::Melee]));
}

#[test]
fn warrior_in_the_back_row_cannot_attack() {
    let bt = battle(&[(10, f(2)), (10, b(2))], &[(18, f(2))]);
    assert!(bt.targets(1).is_empty());
    assert!(bt.helpless(1) && !bt.helpless(0));
}

#[test]
fn long_strike_reaches_the_nearest_on_each_side_through_three_empty_cells() {
    let bt = battle(&[(10, f(2))], &[(18, f(0)), (18, f(5)), (18, f(4)), (18, b(2))]);
    assert_eq!(bt.targets(0), vec![1, 3]);
    assert_eq!(bt.options(0, 3), vec![ActionKind::LongStrike]);
    let bt = battle(&[(10, f(2))], &[(18, f(0)), (18, f(3))]);
    assert_eq!(bt.targets(0), vec![2]);
}

#[test]
fn shooter_reach_from_back_and_front_rows() {
    let bt = battle(&[(11, b(1)), (10, f(3))], &[(18, f(3)), (18, b(4))]);
    assert_eq!(bt.targets(0), vec![2, 3]);
    let bt = battle(&[(11, f(3))], &[(18, f(4)), (18, f(0)), (18, b(3))]);
    assert_eq!(bt.targets(0), vec![1]);
    let bt = battle(&[(11, f(3))], &[(18, f(0)), (18, b(3))]);
    assert_eq!(bt.targets(0), vec![1, 2]);
}

#[test]
fn hostile_mage_reach() {
    let bt = battle(&[(12, b(1)), (10, f(0))], &[(18, f(3)), (18, b(4))]);
    assert_eq!(bt.targets(0), vec![2, 3]);
    let bt = battle(&[(12, f(3))], &[(18, f(3)), (18, b(1))]);
    assert!(bt.targets(0).is_empty(), "front row, enemy opposite: cannot cast");
    let bt = battle(&[(12, f(3))], &[(18, f(0)), (18, b(3))]);
    assert_eq!(bt.targets(0), vec![1, 2]);
}

#[test]
fn a_multi_role_unit_gets_one_action_per_cell_magic_over_shot_over_melee() {
    let all = UnitDef { attack_blow: 30, attack_shot: 20, ..mage(40, 30, MagicSchool::Death, MagicDirection::ToEnemy) };
    // In the front row with an enemy opposite: the mage cannot cast, the shot beats melee.
    let bt = with(vec![all.clone()], &[(40, f(2))], &[(18, f(2))]);
    assert_eq!(bt.options(0, 1), vec![ActionKind::Shot]);
    // Nothing opposite: magic wins.
    let bt = with(vec![all], &[(40, f(0))], &[(18, f(4))]);
    assert_eq!(bt.options(0, 1), vec![ActionKind::Curse]);
}

// --- rows 6, 7: mage actions are automatic ---------------------------------------------------

#[test]
fn row6_hostile_mage_curses_a_fresh_target_then_strikes_it() {
    let caster = acts(41, 2, mage(41, 30, MagicSchool::Life, MagicDirection::ToEnemy));
    let mut bt = with(vec![caster], &[(41, b(2)), (10, f(0))], &[(18, f(2))]);
    assert_eq!(bt.options(0, 2), vec![ActionKind::Curse], "even a Life caster curses first");
    assert_eq!(bt.act_with(2, ActionKind::Strike), Err(ActionError::InvalidTarget), "no choice");
    assert_eq!(bt.act(2).unwrap().kind, ActionKind::Curse);
    assert!(bt.fighters[2].weakened());
    let hit = bt.act(2).unwrap();
    assert_eq!((hit.kind, hit.amount), (ActionKind::Strike, 30));
    to_round(&mut bt, 2);
    assert_eq!(bt.options(0, 2), vec![ActionKind::Curse], "modifiers gone: curse again");
}

#[test]
fn row7_friendly_mage_heals_the_wounded_else_blesses_once_a_turn() {
    let healer = acts(42, 2, mage(42, 20, MagicSchool::Life, MagicDirection::ToAlly));
    let mut bt = with(vec![healer], &[(42, b(2)), (10, f(2)), (10, f(3))], &[(18, f(2))]);
    assert_eq!(bt.options(0, 2), vec![ActionKind::Bless]);
    assert_eq!(bt.options(0, 0), vec![ActionKind::Bless], "itself too");
    bt.fighters[1].hp = 20;
    assert_eq!(bt.options(0, 1), vec![ActionKind::Heal]);
    assert!(bt.options(0, 3).is_empty(), "ToAlly never targets enemies");
    assert_eq!(bt.active(), Some(0));
    let hit = bt.act(1).unwrap();
    assert_eq!((hit.kind, hit.amount, bt.fighters[1].hp), (ActionKind::Heal, 20, 40));
    let hit = bt.act(2).unwrap();
    // Life 20: defence +(3·20/12 + 1) = 6, attack +3·20/24 = 2.
    assert_eq!(hit.buff, Buff { attack: 2, defence: 6, ..Buff::default() });
    assert!(bt.options(0, 2).is_empty(), "blessed this turn and unhurt");
    bt.fighters[2].hp = 30;
    assert_eq!(bt.options(0, 2), vec![ActionKind::Heal], "a blessed unit can still be healed");
    bt.fighters[2].hp = 50;
    to_round(&mut bt, 2);
    assert_eq!(bt.options(0, 2), vec![ActionKind::Bless], "the next turn it can be blessed again");
}

#[test]
fn row7_a_caster_in_the_reserve_tends_only_the_reserve() {
    let c = content_with(vec![], Formation::VANILLA);
    let mut bt = battle_in(&c, &[(13, r(0)), (10, f(1)), (10, r(1))], &[(18, f(1))]);
    bt.fighters[1].hp = 10;
    assert!(bt.options(0, 1).is_empty(), "the front row is out of its reach");
    assert_eq!(bt.options(0, 2), vec![ActionKind::Bless]);
    bt.fighters[2].hp = 10;
    assert_eq!(bt.options(0, 2), vec![ActionKind::Heal]);
}

#[test]
fn elemental_caster_skips_unhurt_elemental_school_allies() {
    let haste = mage(43, 40, MagicSchool::Elemental, MagicDirection::ToAlly);
    let mut bt = with(vec![haste], &[(43, b(2)), (12, b(3))], &[(18, f(2))]);
    assert!(bt.options(0, 1).is_empty());
    bt.fighters[1].hp = 10;
    assert_eq!(bt.options(0, 1), vec![ActionKind::Heal]);
}

// --- rows 1, 2, 3: modifiers last until the next turn; EternalGift --------------------------

#[test]
fn row1_curse_lasts_until_the_next_turn_starts() {
    let pair = acts(98, 2, warrior(98, 30, 5));
    let mut bt = with(vec![pair], &[(18, f(5)), (12, b(1))], &[(98, f(0))]);
    turn_of(&mut bt, 1);
    let hit = bt.act(2).unwrap();
    assert_eq!(hit.kind, ActionKind::Curse);
    // P 40: actions −1, initiative −(1 + 40/7): it acts at threshold 4 with one action.
    assert_eq!((bt.fighters[2].actions, bt.fighters[2].stats[Stat::Initiative]), (1, 4));
    assert_eq!(bt.active(), Some(2));
    to_round(&mut bt, 2);
    assert!(bt.fighters[2].mods.is_empty() && !bt.fighters[2].cursed);
    assert_eq!((bt.fighters[2].actions, bt.fighters[2].stats[Stat::Initiative]), (2, 10));
}

#[test]
fn row2_modifiers_add_up() {
    // A curse (defence −11, attack −3) and a Stun hit (initiative) in one turn add up.
    let mace = bonus(44, Bonus::Stun, warrior(44, 30, 0));
    let mut bt = with(vec![mace], &[(15, b(2)), (44, f(2))], &[(18, f(2))]);
    bt.act(2).unwrap();
    bt.act(2).unwrap();
    assert_eq!(bt.fighters[2].mods, Buff { attack: -3, defence: -11, initiative: -3, actions: 0 });
}

#[test]
fn row3_eternal_gift_changes_battle_stats_stacks_and_its_life_blessing_lowers_defence() {
    let saint = bonus(45, Bonus::EternalGift, acts(45, 2, mage(45, 36, MagicSchool::Life, MagicDirection::ToAll)));
    let mut bt = with(vec![saint], &[(45, b(2)), (10, f(0))], &[(18, f(5))]);
    let hit = bt.act_with(1, ActionKind::Bless).unwrap();
    assert_eq!(hit.buff, Buff { attack: 4, defence: 10, ..Buff::default() });
    let s = &bt.fighters[1].base;
    assert_eq!((s[Stat::AttackBlow], s[Stat::DefenceBlow], s[Stat::DefenceShot]), (34, -5, -5), "the bug");
    assert!(bt.fighters[1].mods.is_empty());
    // Its curses stack: the target never looks weakened, so it is cursed again.
    let hit = bt.act(2).unwrap();
    assert_eq!(hit.kind, ActionKind::Curse);
    to_round(&mut bt, 3);
    turn_of(&mut bt, 0);
    assert_eq!(bt.act(2).unwrap().kind, ActionKind::Curse);
    // Life curses of P 36 and (drained, turn 3) 32: defence −13 − 11, attack −3 − 3.
    let s = &bt.fighters[2].base;
    assert_eq!((s[Stat::AttackBlow], s[Stat::DefenceBlow]), (4, -24));
    assert_eq!(bt.fighters[1].base[Stat::AttackBlow], 34, "still there turns later");
}

#[test]
fn row3_eternal_elemental_blessing_raises_base_initiative() {
    let wind = bonus(46, Bonus::EternalGift, mage(46, 40, MagicSchool::Elemental, MagicDirection::ToAlly));
    let mut bt = with(vec![wind], &[(10, f(0)), (46, b(2))], &[(18, f(5))]);
    bt.skip();
    let hit = bt.act(0).unwrap();
    assert_eq!(hit.buff, Buff { actions: 1, initiative: 6, ..Buff::default() });
    assert_eq!(bt.fighters[0].base[Stat::Initiative], 17);
    assert_eq!(bt.fighters[0].actions, 1, "the extra action is for this turn");
    to_round(&mut bt, 2);
    assert_eq!((bt.fighters[0].stats[Stat::Initiative], bt.fighters[0].actions), (17, 1));
}

// --- rows 4, 5: magic drain -------------------------------------------------------------

#[test]
fn row4_magic_drain_floors_raise_weak_casters_and_undead_death_gets_25() {
    let low = mage(47, 10, MagicSchool::Life, MagicDirection::ToAlly);
    let own = UnitDef { min_magic_power: Some(30), mana_drain: Some(1), ..mage(48, 32, MagicSchool::Elemental, MagicDirection::ToEnemy) };
    let lich = UnitDef { nature: Nature::Undead, ..mage(49, 30, MagicSchool::Death, MagicDirection::ToEnemy) };
    let zero = UnitDef { min_magic_power: Some(0), mana_drain: Some(0), ..mage(50, 20, MagicSchool::Life, MagicDirection::ToAlly) };
    let mut bt = with(
        vec![low, own, lich, zero],
        &[(12, b(1)), (13, b(2)), (14, b(3)), (47, b(4)), (48, r(2)), (49, r(3)), (50, f(0))],
        &[(18, f(5))],
    );
    let powers = |bt: &Battle| (0..7).map(|i| bt.fighters[i].stats[Stat::MagicPower]).collect::<Vec<_>>();
    assert_eq!(powers(&bt), vec![40, 20, 30, 10, 32, 30, 20]);
    let mut seen = vec![];
    for turn in 2..=6 {
        to_round(&mut bt, turn);
        seen.push(powers(&bt));
    }
    assert_eq!(seen[0], vec![35, 18, 28, 15, 31, 28, 18], "the weak Life caster rises to 15");
    assert_eq!(seen[1], vec![30, 16, 26, 15, 30, 26, 16]);
    assert_eq!(seen[4], vec![15, 15, 20, 15, 30, 25, 15], "an ini 0 falls back to the default");
}

#[test]
fn row5_concentration_adds_the_drain_without_a_cap() {
    let focus = bonus(51, Bonus::Concentration, mage(51, 30, MagicSchool::Elemental, MagicDirection::ToEnemy));
    let mut bt = with(vec![focus], &[(51, b(1)), (12, b(2))], &[(18, f(0))]);
    to_round(&mut bt, 2);
    assert_eq!((bt.fighters[0].stats[Stat::MagicPower], bt.fighters[1].stats[Stat::MagicPower]), (35, 35));
    to_round(&mut bt, 15);
    assert_eq!(bt.fighters[0].stats[Stat::MagicPower], 100);
}

// --- rows 8, 9, 10: undead casters, vampirism, the Life curse ---------------------------------

#[test]
fn row8_undead_elemental_and_death_curses_drain_life() {
    let lich = UnitDef { nature: Nature::Undead, ..mage(52, 30, MagicSchool::Death, MagicDirection::ToEnemy) };
    let wraith = UnitDef { nature: Nature::Undead, ..mage(53, 30, MagicSchool::Life, MagicDirection::ToEnemy) };
    let mut bt = with(vec![lich, wraith], &[(52, b(2)), (53, b(3)), (10, f(0))], &[(18, f(2)), (18, f(3))]);
    bt.fighters[0].hp = 10;
    // (30 / CurseMainSpell) / 2 + 1 = 4.
    let hit = bt.act(3).unwrap();
    assert_eq!((hit.kind, hit.amount, bt.fighters[3].hp, bt.fighters[0].hp), (ActionKind::Curse, 4, 196, 14));
    let hit = bt.act(4).unwrap();
    assert_eq!((hit.kind, hit.amount), (ActionKind::Curse, 0), "a Life curse does not drain");
}

#[test]
fn row9_vampirism_on_physical_hits_and_death_strikes_only() {
    let bat = UnitDef { vampirism: 50, ..warrior(54, 30, 0) };
    let ghoul = UnitDef { vampirism: 50, ..acts(55, 2, mage(55, 30, MagicSchool::Death, MagicDirection::ToEnemy)) };
    let saint = UnitDef { vampirism: 50, ..acts(56, 2, mage(56, 30, MagicSchool::Life, MagicDirection::ToEnemy)) };
    let mut bt = with(vec![bat, ghoul, saint], &[(54, f(2)), (55, b(2)), (56, b(3))], &[(18, f(2)), (16, f(3)), (18, f(4))]);
    for i in 0..3 {
        bt.fighters[i].hp = 10;
    }
    bt.fighters[0].crippled = true;
    // Overkill counts: 30 on a 5-HP unit still heals 15. NoHeal does not stop it.
    bt.fighters[3].hp = 5;
    bt.act(3).unwrap();
    assert_eq!(bt.fighters[0].hp, 25);
    bt.act(4).unwrap(); // curse the undead
    bt.act(4).unwrap(); // strike it: no drain from the undead
    assert_eq!(bt.fighters[1].hp, 10);
    bt.act(5).unwrap(); // Life: curse, then strike
    bt.act(5).unwrap();
    assert_eq!(bt.fighters[2].hp, 10, "Life strikes never drain");
    to_round(&mut bt, 2);
    turn_of(&mut bt, 1);
    bt.act(5).unwrap();
    bt.act(5).unwrap(); // a Death strike on a normal unit, power 28 on turn 2: +14
    assert_eq!(bt.fighters[1].hp, 24);
}

#[test]
fn row10_life_curse_uses_the_integer_two_thirds_divisor() {
    let life = mage(57, 36, MagicSchool::Life, MagicDirection::ToAll);
    let bt = with(vec![life], &[(57, b(1))], &[(10, f(0))]);
    // defence −(36 / floor(2·5/3) + 1) = −13, attack −36/10 = −3.
    assert_eq!(bt.curse_buff(0, 1), Buff { attack: -3, defence: -13, ..Buff::default() });
}

#[test]
fn blessings_and_curses_by_school() {
    let life_all = mage(40, 36, MagicSchool::Life, MagicDirection::ToAll);
    let death_all = mage(41, 36, MagicSchool::Death, MagicDirection::ToAll);
    let elem_all = mage(42, 36, MagicSchool::Elemental, MagicDirection::ToAll);
    let bt = with(vec![life_all, death_all, elem_all], &[(40, b(1)), (41, b(2)), (42, b(3)), (10, f(0))], &[(10, f(0)), (16, f(1))]);
    let (ally, foe, undead) = (3, 4, 5);
    assert_eq!(bt.bless_buff(0, ally), Buff { attack: 4, defence: 10, ..Buff::default() });
    assert!(bt.bless_buff(0, undead).is_empty(), "Life does not bless the undead");
    assert_eq!(bt.bless_buff(1, ally), Buff { attack: 7, defence: 3, ..Buff::default() });
    assert_eq!(bt.curse_buff(1, foe), Buff { attack: -8, defence: -3, ..Buff::default() });
    assert_eq!(bt.bless_buff(2, ally), Buff { actions: 1, initiative: 6, ..Buff::default() });
    assert_eq!(bt.curse_buff(2, foe), Buff { actions: -1, initiative: -6, ..Buff::default() });
    assert_eq!([actions_of_power(19), actions_of_power(20), actions_of_power(45), actions_of_power(100)], [0, 1, 2, 3]);
}

#[test]
fn magic_strike_by_school_and_nature() {
    let bt = battle(&[(15, b(1)), (14, b(2)), (12, b(3))], &[(18, f(0)), (16, f(1)), (17, f(2))]);
    let (life, death, elem) = (0, 1, 2);
    let (normal, undead, elemental) = (3, 4, 5);
    assert_eq!([bt.magic_strike(life, normal), bt.magic_strike(life, undead), bt.magic_strike(life, elemental)], [30, 60, 22]);
    assert_eq!([bt.magic_strike(death, normal), bt.magic_strike(death, undead), bt.magic_strike(death, elemental)], [30, 15, 22]);
    assert_eq!([bt.magic_strike(elem, normal), bt.magic_strike(elem, undead)], [30, 30]);
}

#[test]
fn protection_reduces_hostile_magic() {
    let warded = UnitDef { protect_life: 50, protect_elemental: 100, hits: 100, ..warrior(39, 20, 0) };
    let bt = with(vec![warded], &[(15, b(1)), (12, b(2))], &[(39, f(0))]);
    assert_eq!(bt.magic_strike(0, 2), 15);
    assert_eq!(bt.magic_strike(1, 2), 0);
    assert_eq!(bt.options(1, 2), vec![ActionKind::Curse], "the cell is offered whatever the power");
}

#[test]
fn heal_by_school_and_nature() {
    let death_healer = mage(43, 20, MagicSchool::Death, MagicDirection::ToAlly);
    let mut bt = with(vec![death_healer], &[(13, b(1)), (43, b(2)), (10, f(0)), (16, f(1))], &[(18, f(0))]);
    bt.fighters[2].hp = 10;
    bt.fighters[3].hp = 10;
    assert_eq!(bt.preview(0, 2, ActionKind::Heal), Preview::Heal(20));
    assert_eq!(bt.options(0, 3), vec![ActionKind::Bless], "Life cannot heal the undead: it blesses");
    assert_eq!(bt.options(1, 3), vec![ActionKind::Heal], "Death heals the undead");
    assert_eq!(bt.options(1, 2), vec![ActionKind::Bless], "…and only them");
}

// --- rows 11, 12, 13: reserve and collapse ---------------------------------------------------

#[test]
fn row11_front_or_back_row_units_may_enter_the_reserve_once_a_turn() {
    let runner = acts(58, 3, shooter(58, 20));
    let mut bt = with(vec![runner], &[(10, f(2)), (58, b(2)), (10, f(3))], &[(18, f(2))]);
    assert!(bt.moves(0).contains(&r(2)) && bt.moves(0).contains(&r(3)), "from the front row too");
    bt.skip();
    assert_eq!(bt.active(), Some(1));
    bt.move_active(r(2)).unwrap();
    assert!(bt.moves(1).is_empty(), "in the reserve it cannot leave again this turn");
    assert_eq!(bt.move_active(b(2)), Err(ActionError::InvalidTarget));
    to_round(&mut bt, 2);
    turn_of(&mut bt, 1);
    assert!(bt.moves(1).contains(&f(0)) && bt.moves(1).contains(&b(4)), "any cell of rows 1–2");
    bt.move_active(b(4)).unwrap();
    assert!(!bt.moves(1).iter().any(|s| s.row == Reserve), "and not back in the same turn");
}

#[test]
fn reserve_cannot_be_targeted_or_attack() {
    let c = content_with(vec![], Formation::VANILLA);
    let bt = battle_in(&c, &[(11, b(1)), (11, r(1)), (10, f(1))], &[(18, f(1)), (11, r(2))]);
    assert_eq!(bt.targets(0), vec![3], "enemy reserve archer is out of reach");
    assert!(bt.targets(1).is_empty(), "reserve shooter cannot shoot");
    assert!(bt.targets(4).is_empty());
    assert!(bt.moves(1).contains(&f(3)) && bt.moves(1).contains(&b(0)) && !bt.moves(1).contains(&r(0)));
}

#[test]
fn row12_a_voluntary_move_collapses_only_after_the_movers_last_action() {
    let walker = acts(59, 2, warrior(59, 30, 5));
    let mut bt = with(vec![walker], &[(59, f(2)), (11, b(2))], &[(18, f(5))]);
    bt.move_active(b(1)).unwrap();
    assert_eq!((bt.fighters[0].slot, bt.fighters[1].slot), (b(1), b(2)), "front empty, no collapse yet");
    bt.pass();
    assert_eq!((bt.fighters[0].slot, bt.fighters[1].slot), (f(1), f(2)));
}

#[test]
fn row12_an_empty_front_row_is_filled_when_the_battle_starts() {
    // The player's observation of the original; the notes read the collapse only after a
    // death or a unit's last action.
    let bt = battle(&[(11, b(2)), (11, b(3))], &[(18, f(2))]);
    assert_eq!((bt.fighters[0].slot, bt.fighters[1].slot), (f(2), f(3)), "the back row steps forward at once");
    let c = content_with(vec![], Formation::VANILLA);
    let bt = battle_in(&c, &[(11, r(1))], &[(18, f(1)), (11, r(2))]);
    assert_eq!((bt.fighters[0].slot, bt.fighters[2].slot), (f(1), r(2)), "a reserve alone steps forward; one behind a front row stays");
}

#[test]
fn row13_back_row_steps_up_with_its_actions_the_reserve_without() {
    let archer = acts(60, 2, shooter(60, 20));
    let mut bt = with(vec![archer.clone()], &[(10, f(2))], &[(18, f(2)), (60, b(4))]);
    bt.fighters[1].hp = 1;
    bt.act(1).unwrap();
    assert_eq!(bt.fighters[2].slot, f(4));
    assert_eq!((bt.active(), bt.actions_left()), (Some(2), 2), "it keeps its actions");
    let c = content_with(vec![archer], Formation::VANILLA);
    let mut bt = battle_in(&c, &[(10, f(1))], &[(18, f(1)), (60, r(2))]);
    bt.fighters[1].hp = 1;
    bt.act(1).unwrap();
    // It never acts this turn: the next actor is the player's, in turn 2.
    assert_eq!((bt.fighters[2].slot, bt.fighters[2].taken), (f(2), 0), "the reserve steps up and loses its actions");
    assert_eq!((bt.round, bt.active()), (2, Some(0)));
}

// --- row 14: the wide row ------------------------------------------------------------------

#[test]
fn row14_wide_row_blocks_the_back_row_ends_and_moves_units_off_them() {
    let bt = battle(&[(11, b(0)), (11, b(5)), (10, f(2))], &[(18, f(2))]);
    assert!(bt.fighters.iter().all(|f| bt.formation.contains(f.slot)));
    assert_eq!((bt.fighters[0].slot, bt.fighters[1].slot), (b(3), b(2)));
    let c = content_with(vec![], Formation::WIDE);
    let mut bt = prepared(&c, &[(10, f(2)), (11, b(2))], &[(18, f(2))], Team::Player);
    assert_eq!(bt.move_card(b(2), b(0)), Err(ActionError::InvalidTarget), "blocked cell");
    assert_eq!(bt.move_card(b(2), r(4)), Err(ActionError::InvalidTarget));
    bt.move_card(b(2), r(3)).unwrap();
    bt.begin();
    assert!(bt.targets(1).is_empty());
}

// --- rows 15, 16, 17: turn order -----------------------------------------------------------

#[test]
fn row15_16_threshold_scan_ties_to_the_player_who_always_gets_plus_one() {
    let fast = UnitDef { initiative: 12, ..warrior(21, 10, 0) };
    let same = UnitDef { initiative: 12, ..warrior(22, 10, 0) };
    let slow = UnitDef { initiative: 11, ..warrior(23, 10, 0) };
    let c = content_with(vec![fast, same, slow], Formation::WIDE);
    // The enemy attacks; the player's units still get +1: 12 and 13 against 12.
    let mut bt = prepared(&c, &[(23, f(0)), (21, f(1))], &[(22, f(1))], Team::Enemy);
    bt.begin();
    assert_eq!(bt.queue().collect::<Vec<_>>(), vec![1, 0, 2]);
    let mut seen = vec![];
    for _ in 0..3 {
        seen.push(bt.active().unwrap());
        bt.skip();
    }
    assert_eq!(seen, vec![1, 0, 2], "the tie at 12 goes to the player");
}

#[test]
fn row15_turns_start_where_the_first_unit_acted_and_changes_apply_at_once() {
    let quick = UnitDef { initiative: 40, ..warrior(24, 10, 0) };
    let mut bt = with(vec![quick], &[(10, f(0))], &[(24, f(5))]);
    assert_eq!((bt.threshold, bt.active()), (40, Some(1)), "turn 1 starts at 75: the first actor at 40");
    bt.skip();
    bt.skip();
    assert_eq!((bt.round, bt.threshold), (2, 40));
    // An initiative ≤ 0 never acts.
    let frozen = UnitDef { initiative: 0, ..warrior(25, 10, 0) };
    let mut bt = with(vec![frozen], &[(10, f(0))], &[(25, f(5))]);
    for _ in 0..5 {
        assert_eq!(bt.active(), Some(0));
        bt.skip();
    }
}

#[test]
fn row15_a_slowed_unit_acts_later_in_the_same_turn() {
    // Elemental curse −6 initiative: the 10-initiative enemy with two actions now acts at 4,
    // after the other enemy at 8.
    let pair = acts(26, 2, warrior(26, 10, 0));
    let late = UnitDef { initiative: 8, ..warrior(27, 10, 0) };
    let elem = UnitDef { magic_power: 25, ..mage(28, 25, MagicSchool::Elemental, MagicDirection::ToEnemy) };
    let mut bt = with(vec![pair, late, elem], &[(28, b(2))], &[(26, f(0)), (27, f(5))]);
    // P 25: actions −1, initiative −(1 + 25/7) = −4.
    bt.act(1).unwrap();
    assert_eq!(bt.fighters[1].stats[Stat::Initiative], 6);
    assert_eq!(bt.queue().collect::<Vec<_>>(), vec![2, 1]);
}

#[test]
fn row17_artillery_gets_30_initiative_on_turn_1_only() {
    let gun = bonus(24, Bonus::Artillery, UnitDef { initiative: 1, ..shooter(24, 10) });
    let mut bt = with(vec![gun.clone()], &[(10, f(0))], &[(24, b(2))]);
    assert_eq!((bt.active(), bt.fighters[1].stats[Stat::Initiative]), (Some(1), 31));
    to_round(&mut bt, 2);
    assert_eq!((bt.active(), bt.fighters[1].stats[Stat::Initiative]), (Some(0), 1), "not always first");
    let bt = in_building(vec![gun], &[(10, f(0))], &[(24, b(2))], 10);
    assert_eq!(bt.fighters[1].stats[Stat::Initiative], 61, "+60 with building defence ≥ 10");
}

#[test]
fn manevres_and_fast_start() {
    let two = UnitDef { manevres: 2, ..shooter(25, 10) };
    let horse = bonus(26, Bonus::HorseAtack, UnitDef { initiative: 30, ..warrior(26, 10, 0) });
    let mut bt = with(vec![two, horse], &[(26, f(0)), (25, b(1))], &[(18, f(0))]);
    assert_eq!((bt.active(), bt.actions_left()), (Some(0), 2), "HorseAtack: +1 on turn 1");
    bt.skip();
    assert_eq!((bt.active(), bt.actions_left()), (Some(1), 2));
    bt.move_active(b(2)).unwrap();
    assert_eq!((bt.active(), bt.actions_left()), (Some(1), 1), "a step costs one action");
    bt.pass();
    assert_ne!(bt.active(), Some(1), "so does a pass");
    to_round(&mut bt, 2);
    assert_eq!((bt.active(), bt.actions_left()), (Some(0), 1), "no bonus on turn 2");
}

#[test]
fn faster_attack_gives_an_extra_action_on_the_first_two_turns() {
    let fast = bonus(86, Bonus::FasterAttack, UnitDef { initiative: 30, ..warrior(86, 10, 0) });
    let mut bt = with(vec![fast], &[(86, f(0))], &[(18, f(5))]);
    let mut seen = Vec::new();
    for round in 1..=3 {
        to_round(&mut bt, round);
        turn_of(&mut bt, 0);
        seen.push(bt.actions_left());
        bt.skip();
    }
    assert_eq!(seen, vec![2, 2, 1]);
}

// --- rows 18–21: the damage formula ---------------------------------------------------------

#[test]
fn damage_is_attack_minus_defence_at_least_one() {
    let bt = battle(&[(10, f(2)), (18, f(3))], &[(10, f(2))]);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 25);
    assert_eq!(bt.physical_damage(1, 2, ActionKind::Melee), 5);
    let armour = UnitDef { hits: 100, ..warrior(27, 1, 50) };
    let bt = with(vec![armour], &[(10, f(2))], &[(27, f(2))]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 1);
}

#[test]
fn back_row_gets_row2_def_against_shots_only() {
    let mixed = UnitDef { hits: 100, ..warrior(28, 10, 4) };
    let bt = with(vec![mixed], &[(11, b(1)), (10, f(1))], &[(28, f(1)), (28, b(3))]);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Shot), 16);
    assert_eq!(bt.physical_damage(0, 3, ActionKind::Shot), 11, "+5 in the back row");
    assert_eq!(bt.physical_damage(1, 3, ActionKind::Melee), 26, "melee unaffected");
}

/// 30 melee and 30 ranged attack with bonus `b`.
fn both(id: u32, b: Option<Bonus>) -> UnitDef {
    UnitDef { attack_shot: 30, bonus: b, ..warrior(id, 30, 0) }
}

#[test]
fn row17_18_19_piercing_differs_between_melee_and_shots() {
    let set = [
        (Bonus::ArmorIgnore, true, true),
        (Bonus::PoisonArmorIgnore, true, true),
        (Bonus::VampirsGist, true, false),
        (Bonus::OldVampirsGist, true, false),
        (Bonus::Artillery, false, true),
    ];
    for (k, (bn, melee, shot)) in set.into_iter().enumerate() {
        let id = 100 + k as u32;
        let mut bt = with(vec![both(id, Some(bn.clone())), armour()], &[(id, f(2))], &[(84, f(2)), (84, b(2))]);
        bt.set_building_defence(Team::Enemy, 3);
        let (m, s, back) = (bt.physical_damage(0, 1, ActionKind::Melee), bt.physical_damage(0, 1, ActionKind::Shot), bt.physical_damage(0, 2, ActionKind::Shot));
        assert_eq!(m, if melee { 27 } else { 7 }, "{bn:?} melee");
        assert_eq!(s, if shot { 27 } else { 7 }, "{bn:?} shot");
        // A piercing shot still faces Row2Def and the building.
        assert_eq!(back, if shot { 22 } else { 2 }, "{bn:?} shot at the back row");
    }
}

#[test]
fn row20_spear_defense_and_long_strike_apply_before_the_building() {
    let spear = bonus(29, Bonus::SpearDefense, UnitDef { hits: 300, initiative: 1, ..warrior(29, 5, 8) });
    let mut bt = in_building(vec![spear], &[(10, f(2)), (11, b(2))], &[(29, f(2))], 2);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 4, "30 − (8·3 + 2)");
    assert_eq!(bt.physical_damage(1, 2, ActionKind::Shot), 10, "shots: 20 − (8 + 2)");
    to_round(&mut bt, 2);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 20);
    let tough = UnitDef { hits: 500, ..warrior(19, 1, 20) };
    let bt = in_building(vec![tough], &[(10, f(0))], &[(19, f(4))], 10);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::LongStrike), 10, "30 − (20/2 + 10)");
}

#[test]
fn long_strike_halves_defence_and_flank_strike_doubles_attack() {
    let tough = UnitDef { hits: 500, ..warrior(19, 1, 20) };
    let flanker = bonus(20, Bonus::FlankStrike, warrior(20, 30, 5));
    let bt = with(vec![tough.clone(), flanker.clone()], &[(10, f(2))], &[(19, f(2))]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 10);
    let bt = with(vec![tough.clone(), flanker.clone()], &[(10, f(0))], &[(19, f(4))]);
    assert_eq!(bt.options(0, 1), vec![ActionKind::LongStrike]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::LongStrike), 20);
    let bt = with(vec![tough, flanker], &[(20, f(0))], &[(19, f(4))]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::LongStrike), 50);
}

#[test]
fn row21_knight_army_takes_80_percent_physical_damage_even_with_the_hero_down() {
    let mut bt = battle(&[(1, f(0)), (18, f(2))], &[(10, f(2)), (15, b(2))]);
    assert_eq!(bt.physical_damage(2, 1, ActionKind::Melee), 24);
    assert_eq!(bt.magic_strike(3, 1), 30, "magic is not reduced");
    bt.fighters[0].hp = 0;
    assert_eq!(bt.physical_damage(2, 1, ActionKind::Melee), 24);
    let bt = battle(&[(10, f(0)), (18, f(2))], &[(10, f(2))]);
    assert_eq!(bt.physical_damage(2, 1, ActionKind::Melee), 30);
}

#[test]
fn armor_ignore_skips_defence_but_not_the_building() {
    let pierce = bonus(30, Bonus::ArmorIgnore, warrior(30, 30, 0));
    let armour = UnitDef { hits: 100, ..warrior(27, 1, 20) };
    let mut bt = with(vec![pierce, armour], &[(30, f(2))], &[(27, f(2))]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 30);
    bt.set_building_defence(Team::Enemy, 6);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 24);
}

#[test]
fn evasive_dead_vs_shots_god_anger_and_evasion() {
    let evasive = bonus(33, Bonus::Evasive, UnitDef { hits: 100, ..warrior(33, 1, 0) });
    let corpse = bonus(34, Bonus::Dead, UnitDef { hits: 100, ..warrior(34, 1, 0) });
    let angry = bonus(35, Bonus::GodAnger, shooter(35, 20));
    let dodger = UnitDef { evasion: Some(50), hits: 100, ..warrior(36, 1, 0) };
    let bt = with(vec![evasive, corpse, angry, dodger], &[(11, b(1)), (35, b(2))], &[(33, f(0)), (34, f(1)), (36, f(2))]);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Shot), 13, "20 × 2/3");
    assert_eq!(bt.physical_damage(0, 3, ActionKind::Shot), 6, "20 × 3/10");
    assert_eq!(bt.physical_damage(1, 3, ActionKind::Shot), 16, "6 + 10 GodAnger");
    assert_eq!(bt.physical_damage(0, 4, ActionKind::Shot), 10, "Evasion 50%");
}

// --- rows 22, 23, 24 ----------------------------------------------------------------------------

#[test]
fn row22_any_counterblow_unit_answers_melee_even_without_attack() {
    let chief = bonus(37, Bonus::Counterblow, UnitDef { hits: 100, ..warrior(37, 25, 0) });
    let archer = bonus(38, Bonus::Counterblow, UnitDef { hits: 300, ..shooter(38, 20) });
    let mut bt = with(vec![chief, archer], &[(10, f(2)), (10, f(4)), (11, b(3))], &[(37, f(2)), (38, f(4))]);
    let hit = bt.act(3).unwrap();
    assert_eq!((hit.amount, hit.counter), (30, Some(20)), "25 − 5 back");
    let hit = bt.act(4).unwrap();
    assert_eq!(hit.counter, Some(1), "a unit without melee attack answers for 1");
    let hit = bt.act(4).unwrap();
    assert_eq!(hit.counter, None, "not after a shot");
}

#[test]
fn row23_a_ghosts_killer_dies_only_below_30_death_protection_per_ghost_action() {
    let ghost = bonus(32, Bonus::Ghost, UnitDef { hits: 1, initiative: 1, ..warrior(32, 1, 0) });
    let blessed = UnitDef { protect_death: 30, ..warrior(31, 30, 5) };
    let mut bt = with(vec![ghost.clone(), blessed.clone()], &[(10, f(2)), (31, f(3))], &[(32, f(2)), (32, f(3))]);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 1, "a ghost takes 1");
    let hit = bt.act(2).unwrap();
    assert!(hit.killed && hit.actor_died);
    let hit = bt.act(3).unwrap();
    assert!(hit.killed && !hit.actor_died, "Death protection 30 ≥ 30 × 1");
    let old = bonus(33, Bonus::Ghost, UnitDef { hits: 1, manevres: 2, initiative: 1, ..warrior(33, 1, 0) });
    let mut bt = with(vec![old, blessed], &[(31, f(3))], &[(33, f(3))]);
    assert!(bt.act(1).unwrap().actor_died, "30 < 60");
}

#[test]
fn unvulnerable_and_death_curse() {
    let stone = bonus(31, Bonus::Unvulnerabe, UnitDef { hits: 3, ..warrior(31, 1, 0) });
    let cursed = bonus(38, Bonus::DeathCurse, UnitDef { hits: 1, ..warrior(38, 1, 0) });
    let mut bt = with(vec![stone, cursed], &[(10, f(2)), (15, b(2))], &[(31, f(2)), (38, f(3))]);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 1);
    let hit = bt.act(3).unwrap();
    assert!(hit.killed && hit.actor_died);
}

#[test]
fn row24_garrison_doubles_ab_db_ds_and_the_community_fix_adds_as_at_exactly_10() {
    let keep = bonus(90, Bonus::Garrison, UnitDef { attack_shot: 10, hits: 300, ..warrior(90, 20, 5) });
    let bt = in_building(vec![keep.clone(), hammer()], &[(69, f(2))], &[(90, f(2))], 10);
    let s = &bt.fighters[1].base;
    assert_eq!((s[Stat::AttackBlow], s[Stat::AttackShot], s[Stat::DefenceBlow], s[Stat::DefenceShot]), (40, 10, 10, 10));
    assert_eq!((bt.fighters[1].stats[Stat::AttackBlow], bt.fighters[1].stats[Stat::AttackShot]), (50, 20), "+AS each turn");
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 53, "(100 − 20) × 2/3");
    let bt = in_building(vec![keep.clone(), hammer()], &[(69, f(2))], &[(90, f(2))], 12);
    assert_eq!(bt.fighters[1].stats[Stat::AttackBlow], 40, "only at exactly 10");
    let bt = in_building(vec![keep, hammer()], &[(69, f(2))], &[(90, f(2))], 6);
    assert_eq!((bt.fighters[1].stats[Stat::AttackBlow], bt.physical_damage(0, 1, ActionKind::Melee)), (20, 89));
}

// --- rows 25, 26: regeneration and poison -------------------------------------------------------

#[test]
fn row25_regeneration_rounds_half_even_with_no_minimum() {
    let slow = UnitDef { regen: 1, ..warrior(61, 1, 0) };
    let mid = UnitDef { regen: 5, ..warrior(62, 1, 0) };
    let mut bt = with(vec![slow, mid], &[(61, f(0)), (62, f(1))], &[(18, f(5))]);
    bt.fighters[0].hp = 40;
    bt.fighters[1].hp = 40;
    to_round(&mut bt, 2);
    assert_eq!((bt.fighters[0].hp, bt.fighters[1].hp), (40, 42), "0.5 → 0, 2.5 → 2");
}

#[test]
fn row26_poisons_set_regeneration_and_replace_the_units_own() {
    let viper = bonus(71, Bonus::Poison, warrior(71, 30, 0));
    let cobra = bonus(72, Bonus::PoisonS, warrior(72, 30, 0));
    let witch = bonus(73, Bonus::Poison, mage(73, 30, MagicSchool::Life, MagicDirection::ToEnemy));
    let weak = bonus(74, Bonus::Poison, mage(74, 15, MagicSchool::Life, MagicDirection::ToEnemy));
    let troll = UnitDef { regen: 10, hits: 200, ..warrior(88, 1, 0) };
    let mut bt = with(
        vec![viper, cobra, witch, weak, troll],
        &[(71, f(1)), (72, f(2)), (73, b(1)), (74, b(2))],
        &[(88, f(1)), (18, f(2)), (18, f(3)), (18, f(4))],
    );
    bt.act(4).unwrap();
    bt.act(5).unwrap();
    bt.act(6).unwrap(); // a curse of power 30 > 15 poisons
    bt.act(7).unwrap(); // power 15 does not
    assert_eq!([4, 5, 6, 7].map(|i| bt.fighters[i].regen), [-20, -25, -20, 0]);
    to_round(&mut bt, 2);
    assert_eq!([4, 5, 6].map(|i| bt.fighters[i].hp), [200 - 30 - 40, 200 - 30 - 50, 200 - 40], "20% and 25% a turn");
}

#[test]
fn row26_poison_armor_ignore_pierces_and_keeps_the_worst_poison() {
    let sting = bonus(85, Bonus::PoisonArmorIgnore, warrior(85, 30, 0));
    let viper = bonus(71, Bonus::Poison, warrior(71, 30, 0));
    let mut bt = in_building(vec![sting, viper, armour()], &[(85, f(2)), (71, f(3))], &[(84, f(2)), (84, f(3))], 6);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 24, "the building still counts");
    bt.act(2).unwrap();
    assert_eq!(bt.fighters[2].regen, -10);
    bt.act(3).unwrap();
    to_round(&mut bt, 2);
    bt.act(3).unwrap();
    assert_eq!(bt.fighters[3].regen, -20, "min(−20, −10)");
}

#[test]
fn row34_ctr_poison_lowers_the_melee_strikers_regeneration_by_20_a_hit() {
    let toad = bonus(65, Bonus::CtrPoison, UnitDef { hits: 300, ..warrior(65, 1, 0) });
    let brute = acts(66, 2, warrior(66, 30, 0));
    let mut bt = with(vec![toad, brute], &[(66, f(2)), (11, b(2))], &[(65, f(2))]);
    bt.act(2).unwrap();
    bt.act(2).unwrap();
    assert_eq!(bt.fighters[0].regen, -40, "stacks");
    bt.act(2).unwrap();
    assert_eq!(bt.fighters[1].regen, 0, "a shot does not touch it");
}

// --- rows 27, 28: the end ----------------------------------------------------------------------

#[test]
fn row27_the_first_action_of_turn_25_ends_it_as_a_victory() {
    let mut bt = battle(&[(10, f(0))], &[(18, f(5))]);
    to_round(&mut bt, 25);
    assert_eq!(bt.outcome(), Outcome::Ongoing);
    bt.pass();
    assert_eq!((bt.outcome(), bt.round, bt.end_reason()), (Outcome::Victory, 25, Some(EndReason::TurnLimit)));
    assert!(!bt.player_xp().is_empty(), "a victory pays XP");
}

#[test]
fn row28_a_side_of_only_surrendering_units_gives_up_and_only_they_pray() {
    let priest = UnitDef { surrender: 20, ..unit(91, "priest") };
    let nun = UnitDef { surrender: 15, ..unit(92, "nun") };
    let mut bt = with(vec![priest, nun], &[(10, f(2))], &[(18, f(2)), (91, f(3)), (92, b(2))]);
    bt.fighters[2].hp = 1;
    assert!(bt.act(2).unwrap().killed);
    assert_eq!(bt.outcome(), Outcome::Ongoing, "the bag still fights");
    turn_of(&mut bt, 0);
    bt.fighters[1].hp = 1;
    bt.act(1).unwrap();
    assert_eq!((bt.outcome(), bt.end_reason()), (Outcome::Victory, Some(EndReason::Surrender(Team::Enemy))));
    assert_eq!(bt.surrender_mana(Team::Player), 15, "the priest killed before gives nothing");
    assert!(bt.fighters[3].surrendered && !bt.fighters[3].alive());
}

#[test]
fn row28_a_lone_surrendering_garrison_gives_up_after_the_first_action() {
    let priest = UnitDef { surrender: 20, ..unit(91, "priest") };
    let mut bt = with(vec![priest.clone()], &[(10, f(2))], &[(91, f(2))]);
    assert_eq!(bt.outcome(), Outcome::Ongoing);
    bt.pass();
    assert_eq!((bt.outcome(), bt.surrender_mana(Team::Player)), (Outcome::Victory, 20));
    // The player's side surrenders too.
    let mut bt = with(vec![priest], &[(91, b(2))], &[(18, f(2))]);
    bt.pass();
    assert_eq!((bt.outcome(), bt.end_reason()), (Outcome::Defeat, Some(EndReason::Surrender(Team::Player))));
}

// --- row 29: one bonus per unit --------------------------------------------------------------

#[test]
fn row29_an_items_bonus_overwrites_the_units() {
    let chief = bonus(37, Bonus::Counterblow, warrior(37, 25, 0));
    let ring = crate::rules::content::ArtefactDef { bonus: Some(Bonus::ArmorIgnore), ..item(900, ArtefactType::Ring) };
    let mut c = content(vec![chief, armour()], vec![ring]);
    c.formation = Formation::WIDE;
    let c = Arc::new(c);
    let mut u = Unit::new(&c, UnitId(37), f(2));
    u.items[0] = Some(ItemId(900));
    let squad = [u];
    let p: Vec<_> = squad.iter().enumerate().collect();
    let mut bt = Battle::new(c.clone(), &p, &[Unit::new(&c, UnitId(84), f(2))], Team::Player);
    bt.begin();
    assert_eq!(bt.fighters[0].base.bonuses, vec![Bonus::ArmorIgnore]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 25);
}

// --- rows 31–50: Community bonuses ------------------------------------------------------------

#[test]
fn row31_hunger_heals_on_a_melee_kill_and_at_turn_start_after_a_death() {
    let hungry = bonus(60, Bonus::Hunger, warrior(60, 30, 0));
    let mut bt = with(vec![hungry.clone()], &[(60, f(2))], &[(10, f(2)), (10, f(3))]);
    bt.fighters[0].hp = 10;
    bt.fighters[1].hp = 1;
    assert!(bt.act(1).unwrap().killed);
    assert_eq!(bt.fighters[0].hp, 50);
    let gunner = bonus(63, Bonus::Hunger, shooter(63, 30));
    let mut bt = with(vec![gunner], &[(63, b(2)), (10, f(0))], &[(10, f(2)), (10, f(3))]);
    bt.fighters[0].hp = 10;
    bt.fighters[2].hp = 1;
    assert!(bt.act(2).unwrap().killed);
    assert_eq!(bt.fighters[0].hp, 10, "no heal for a shot kill");
    to_round(&mut bt, 2);
    assert_eq!(bt.fighters[0].hp, 50, "the unit count changed: healed at the turn start");
    bt.fighters[0].hp = 10;
    to_round(&mut bt, 3);
    assert_eq!(bt.fighters[0].hp, 10, "nothing changed since");
}

#[test]
fn row32_berserk_sets_an_attack_modifier_from_the_hp_lost() {
    let berserk = bonus(61, Bonus::Berserk, warrior(61, 30, 0));
    let mut bt = with(vec![berserk], &[(10, f(2))], &[(61, f(2))]);
    assert_eq!(bt.fighters[1].stats[Stat::AttackBlow], 30);
    bt.fighters[1].mods.attack = 7; // as if blessed
    bt.act(1).unwrap(); // 30 → 20 HP left
    assert_eq!(bt.fighters[1].mods.attack, 13, "30·75·30/50/100, overwriting the blessing");
    bt.fighters[1].hp = 25;
    to_round(&mut bt, 2);
    assert_eq!(bt.fighters[1].stats[Stat::AttackBlow], 30 + 11, "recomputed at the turn start");
}

#[test]
fn row33_exhaustion_takes_10_points_of_every_protection_per_spell() {
    let tired = bonus(62, Bonus::Exhaustion, mage(62, 30, MagicSchool::Life, MagicDirection::ToEnemy));
    let warded = UnitDef { protect_life: 50, protect_death: 5, hits: 300, ..warrior(39, 1, 0) };
    let mut bt = with(vec![tired, warded], &[(62, b(1)), (10, f(5))], &[(39, f(0))]);
    assert_eq!(bt.magic_strike(0, 2), 15);
    bt.act(2).unwrap();
    let s = &bt.fighters[2].stats;
    assert_eq!((s[Stat::ProtectLife], s[Stat::ProtectDeath], s[Stat::ProtectElemental]), (40, 0, 0));
    assert_eq!(bt.magic_strike(0, 2), 18, "30 × 60%");
}

#[test]
fn drying_takes_eight_percent_of_max_hp_through_any_protection() {
    let dry = bonus(63, Bonus::Drying, mage(63, 30, MagicSchool::Life, MagicDirection::ToEnemy));
    let warded = UnitDef { protect_life: 100, hits: 300, ..warrior(39, 1, 0) };
    let mut bt = with(vec![dry, warded], &[(63, b(1)), (10, f(5))], &[(39, f(0))]);
    assert_eq!(bt.magic_strike(0, 2), 24, "0 + 8% of 300");
    let hit = bt.act(2).unwrap();
    assert_eq!((hit.kind, hit.amount, bt.fighters[2].hp), (ActionKind::Curse, 24, 276), "a curse dries too");
}

#[test]
fn suicide_unit_dies_after_its_attack() {
    let bomber = bonus(66, Bonus::Suicide, warrior(66, 30, 0));
    let mut bt = with(vec![bomber], &[(66, f(2)), (10, f(3))], &[(18, f(2))]);
    let hit = bt.act(2).unwrap();
    assert_eq!((hit.amount, hit.actor_died, bt.fighters[0].hp), (30, true, 0));
}

#[test]
fn row35_splash_80_40_on_melee_shots_and_heals_in_battles_on_screen_only() {
    let sweep = bonus(67, Bonus::Splash, warrior(67, 50, 0));
    let mut bt = with(vec![sweep.clone()], &[(67, f(2))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(5)), (18, b(2))]);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 40);
    let hit = bt.act(2).unwrap();
    assert_eq!((hit.amount, hit.splash.clone()), (40, vec![(1, 20), (3, 20)]));
    assert_eq!([bt.fighters[4].hp, bt.fighters[5].hp], [200, 200]);
    // A melee neighbour must be within one column of the attacker too.
    let mut bt = with(vec![sweep.clone()], &[(67, f(1))], &[(18, f(1)), (18, f(2)), (18, f(3))]);
    assert_eq!(bt.act(2).unwrap().splash, vec![(1, 20)]);
    // Shots: both neighbours.
    let gun = bonus(68, Bonus::Splash, shooter(68, 50));
    let mut bt = with(vec![gun], &[(68, b(2)), (10, f(0))], &[(18, f(3)), (18, f(4)), (18, f(5))]);
    assert_eq!(bt.act(3).unwrap().splash, vec![(2, 20), (4, 20)]);
    // Heals: 80% and 40%.
    let medic = bonus(64, Bonus::Splash, mage(64, 20, MagicSchool::Life, MagicDirection::ToAlly));
    let mut bt = with(vec![medic], &[(64, b(2)), (10, f(1)), (10, f(2)), (10, f(3))], &[(18, f(5))]);
    for i in 1..4 {
        bt.fighters[i].hp = 10;
    }
    let hit = bt.act(2).unwrap();
    assert_eq!((hit.kind, hit.amount, hit.splash), (ActionKind::Heal, 16, vec![(1, 8), (3, 8)]));
    // Off screen (AI against AI): no splash.
    let mut bt = with(vec![sweep], &[(67, f(2))], &[(18, f(1)), (18, f(2)), (18, f(3))]);
    bt.set_simulation();
    let hit = bt.act(2).unwrap();
    assert_eq!((hit.amount, hit.splash), (50, vec![]));
}

#[test]
fn row36_fortify_adds_a_quarter_of_defence_per_turn_after_the_first() {
    let wall = bonus(68, Bonus::Fortify, UnitDef { hits: 3000, ..warrior(68, 1, 20) });
    let mut bt = with(vec![wall, hammer()], &[(69, f(2)), (11, b(2))], &[(68, f(2))]);
    let mut seen = Vec::new();
    for round in 1..=7 {
        to_round(&mut bt, round);
        seen.push(bt.physical_damage(0, 2, ActionKind::Melee));
    }
    assert_eq!(seen, vec![80, 75, 70, 65, 60, 55, 55]);
    assert_eq!(bt.physical_damage(1, 2, ActionKind::Shot), 1, "both defences: 20 − 45");
}

#[test]
fn row37_dominate_does_nothing() {
    let lord = bonus(70, Bonus::Dominate, UnitDef { hits: 100, ..warrior(70, 30, 0) });
    let bt = with(vec![lord], &[(70, f(2))], &[(18, f(2)), (10, f(3))]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 30);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 25);
}

#[test]
fn row38_potent_skips_protection_and_the_nature_multipliers() {
    let potent = bonus(74, Bonus::Potent, mage(74, 30, MagicSchool::Life, MagicDirection::ToEnemy));
    let warded = UnitDef { protect_life: 100, hits: 300, ..warrior(39, 1, 0) };
    let bt = with(vec![potent, warded], &[(74, b(1)), (15, b(2))], &[(39, f(0)), (16, f(1)), (17, f(2))]);
    assert_eq!((bt.magic_strike(0, 2), bt.magic_strike(1, 2)), (30, 0));
    assert_eq!((bt.magic_strike(0, 3), bt.magic_strike(0, 4)), (30, 30), "no ×2 on undead, no ¾ on elementals");
}

#[test]
fn row39_stun_cuts_30_percent_of_current_initiative_per_hit_this_turn() {
    let mace = bonus(75, Bonus::Stun, acts(75, 2, warrior(75, 30, 0)));
    let mut bt = with(vec![mace], &[(75, f(2))], &[(18, f(2))]);
    bt.act(1).unwrap();
    assert_eq!(bt.fighters[1].stats[Stat::Initiative], 7);
    bt.act(1).unwrap();
    assert_eq!(bt.fighters[1].stats[Stat::Initiative], 5, "cumulative: 7 − 2");
    to_round(&mut bt, 2);
    assert_eq!(bt.fighters[1].stats[Stat::Initiative], 10);
}

#[test]
fn row40_first_shot_gets_30_initiative_on_turn_1() {
    let quick = bonus(76, Bonus::FirstShot, UnitDef { initiative: 1, ..warrior(76, 10, 0) });
    let gun = bonus(24, Bonus::Artillery, UnitDef { initiative: 1, ..shooter(24, 10) });
    let mut bt = with(vec![quick.clone(), gun], &[(10, f(0))], &[(76, f(0)), (24, b(2))]);
    assert_eq!(bt.queue().collect::<Vec<_>>(), vec![1, 2, 0]);
    to_round(&mut bt, 2);
    assert_eq!(bt.queue().collect::<Vec<_>>(), vec![0, 1, 2]);
    let bt = in_building(vec![quick], &[(10, f(0))], &[(76, f(0))], 10);
    assert_eq!(bt.fighters[1].stats[Stat::Initiative], 61);
}

#[test]
fn row41_bastion_doubles_its_attacks_and_defences_every_turn_anywhere() {
    let tower = bonus(77, Bonus::Bastion, UnitDef { hits: 3000, attack_shot: 3, ..warrior(77, 10, 5) });
    let mut bt = with(vec![tower], &[(10, f(0))], &[(77, f(5))]);
    let stats = |bt: &Battle| {
        let s = &bt.fighters[1].stats;
        [s[Stat::AttackBlow], s[Stat::AttackShot], s[Stat::DefenceBlow], s[Stat::DefenceShot]]
    };
    assert_eq!(stats(&bt), [20, 6, 10, 10]);
    to_round(&mut bt, 3);
    assert_eq!(stats(&bt), [80, 24, 40, 40]);
}

#[test]
fn row42_flying_melees_the_three_front_cells_from_either_row() {
    let bird = bonus(79, Bonus::Flying, warrior(79, 30, 0));
    let bt = with(vec![bird.clone()], &[(79, b(2)), (10, f(0))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(5)), (18, b(2))]);
    assert_eq!(bt.targets(0), vec![2, 3, 4]);
    assert_eq!(bt.options(0, 3), vec![ActionKind::Melee]);
    let c = content_with(vec![bird], Formation::VANILLA);
    let bt = battle_in(&c, &[(79, r(0)), (10, f(0))], &[(18, f(0))]);
    assert!(bt.targets(0).is_empty(), "not from the reserve");
    // A flying shooter shoots as before and gains a melee cell only where it cannot shoot.
    let hawk = bonus(80, Bonus::Flying, shooter(80, 20));
    let bt = with(vec![hawk], &[(80, b(2)), (10, f(0))], &[(18, f(2)), (18, b(3))]);
    assert_eq!((bt.options(0, 2), bt.options(0, 3)), (vec![ActionKind::Shot], vec![ActionKind::Shot]));
}

#[test]
fn row43_bleeding_costs_75_percent_of_attack_and_power_at_each_action_start() {
    let knife = bonus(81, Bonus::Bleed, warrior(81, 30, 0));
    let mut bt = with(vec![knife], &[(81, f(2))], &[(18, f(2)), (18, f(3))]);
    bt.act(1).unwrap();
    assert_eq!((bt.fighters[1].bleed, bt.fighters[1].hp), (75, 170));
    assert_eq!(bt.active(), Some(1));
    bt.pass();
    assert_eq!(bt.fighters[1].hp, 163, "(10 + 0 + 0) × 75%");
    to_round(&mut bt, 2);
    turn_of(&mut bt, 1);
    bt.fighters[1].hp = 5;
    let before = bt.fighters[0].hp;
    let hit = bt.act(0).unwrap();
    assert!(hit.actor_died && !bt.fighters[1].alive(), "bled to death before striking");
    assert_eq!((hit.amount, bt.fighters[0].hp), (0, before));
}

#[test]
fn row44_preventive_strike_answers_first() {
    let guard = bonus(89, Bonus::PreventiveStrike, UnitDef { hits: 100, ..warrior(89, 60, 0) });
    let archer = bonus(47, Bonus::PreventiveStrike, UnitDef { hits: 100, ..shooter(47, 20) });
    let mut bt = with(vec![guard, archer], &[(10, f(2)), (10, f(3)), (11, b(2)), (15, b(3))], &[(89, f(2)), (47, f(3))]);
    let hit = bt.act(4).unwrap();
    assert_eq!((hit.counter, hit.actor_died, hit.amount, bt.fighters[4].hp), (Some(50), true, 0, 100), "55 kills the striker first");
    let hit = bt.act(5).unwrap();
    assert_eq!((hit.counter, hit.amount), (Some(15), 30), "no melee attack: it shoots first");
    let hit = bt.act(4).unwrap();
    assert_eq!(hit.counter, None, "a shot on the guard: it has no shot to answer with");
    let hit = bt.act(5).unwrap();
    assert_eq!((hit.kind, hit.counter), (ActionKind::Curse, Some(15)), "a spell on the archer: it shoots (20 − Row2Def)");
}

#[test]
fn row45_flock_moves_the_attack_by_a_quarter_by_the_starting_army_sizes() {
    let wolf = bonus(80, Bonus::Flock, warrior(80, 40, 0));
    let mut bt = with(vec![wolf.clone()], &[(80, f(2)), (18, f(3))], &[(18, f(2))]);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 50);
    bt.fighters[1].hp = 0;
    to_round(&mut bt, 2);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 50, "the sizes at the start count");
    let bt = with(vec![wolf.clone()], &[(80, f(2))], &[(18, f(2)), (18, f(3))]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 30);
    let bt = with(vec![wolf], &[(80, f(2))], &[(18, f(2))]);
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 40);
}

#[test]
fn row46_armor_breaker_cuts_defence_by_a_quarter_per_hit() {
    let breaker = bonus(83, Bonus::ArmorBreaker, warrior(83, 30, 0));
    let mut bt = with(vec![breaker, armour()], &[(83, f(2))], &[(84, f(2))]);
    assert_eq!(bt.act(1).unwrap().amount, 10);
    assert_eq!((bt.fighters[1].stats[Stat::DefenceBlow], bt.fighters[1].stats[Stat::DefenceShot]), (15, 15));
    assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 15);
}

#[test]
fn row47_no_heal_blocks_heals_blessings_and_regeneration() {
    let cripple = bonus(87, Bonus::NoHeal, warrior(87, 30, 0));
    let troll = UnitDef { regen: 10, hits: 200, ..warrior(88, 1, 0) };
    let mut bt = with(vec![cripple, troll], &[(87, f(2))], &[(88, f(2)), (13, b(1))]);
    bt.act(1).unwrap();
    assert!(bt.fighters[1].crippled);
    assert!(bt.options(2, 1).is_empty(), "neither heal nor bless");
    to_round(&mut bt, 2);
    assert_eq!(bt.fighters[1].hp, 170, "no regeneration");
}

#[test]
fn row48_killing_strike_at_a_quarter_and_fate_gift_can_still_save() {
    let axe = bonus(92, Bonus::KillingStrike, warrior(92, 30, 0));
    let lucky = bonus(95, Bonus::FateGift, UnitDef { hits: 40, ..warrior(95, 20, 4) });
    let mut bt = with(vec![axe, lucky], &[(92, f(2))], &[(18, f(2)), (18, f(3)), (95, f(1))]);
    bt.fighters[1].hp = 80;
    bt.fighters[2].hp = 100;
    let hit = bt.act(1).unwrap();
    assert_eq!((hit.killed, hit.amount), (true, 30), "50 left of 200 is 25%");
    turn_of(&mut bt, 0);
    let hit = bt.act(2).unwrap();
    assert_eq!((hit.killed, bt.fighters[2].hp), (false, 70));
    turn_of(&mut bt, 0);
    bt.fighters[3].hp = 36;
    let hit = bt.act(3).unwrap();
    assert!(!hit.killed, "36 − 26 = 10 ≤ 25%: finished, but fate saves it");
    assert_eq!(bt.fighters[3].hp, 48);
}

#[test]
fn row49_assault_doubles_on_turn_1_against_a_strong_building_and_takes_two_thirds() {
    let sapper = bonus(78, Bonus::Assault, UnitDef { hits: 300, ..warrior(78, 20, 5) });
    let bt = in_building(vec![sapper.clone(), hammer()], &[(78, f(2))], &[(69, f(2))], 10);
    let s = &bt.fighters[0].stats;
    assert_eq!((s[Stat::AttackBlow], s[Stat::DefenceBlow]), (40, 10));
    assert_eq!(bt.physical_damage(1, 0, ActionKind::Melee), 60, "(100 − 10) × 2/3");
    let bt = in_building(vec![sapper.clone(), hammer()], &[(78, f(2))], &[(69, f(2))], 5);
    assert_eq!(bt.fighters[0].stats[Stat::AttackBlow], 20, "a weak building: no doubling");
    let bt = with(vec![sapper, hammer()], &[(78, f(2))], &[(69, f(2))]);
    assert_eq!(bt.physical_damage(1, 0, ActionKind::Melee), 95);
}

#[test]
fn row50_fate_gift_refills_and_strengthens_once() {
    let lucky = bonus(95, Bonus::FateGift, UnitDef { hits: 40, ..warrior(95, 20, 4) });
    let mut bt = with(vec![lucky.clone(), hammer()], &[(69, f(2))], &[(95, f(2))]);
    let hit = bt.act(1).unwrap();
    assert!(!hit.killed);
    let t = &bt.fighters[1];
    assert_eq!((t.hp, t.max_hp(), t.actions, t.regen), (48, 48, 1, 20));
    assert_eq!((t.stats[Stat::ProtectLife], t.stats[Stat::ProtectDeath], t.stats[Stat::Initiative]), (20, 20, 15));
    assert!(t.base.bonuses.is_empty(), "used up");
    turn_of(&mut bt, 0);
    assert!(bt.act(1).unwrap().killed, "only once");
    // Poison deaths are not saved.
    let viper = bonus(71, Bonus::Poison, warrior(71, 10, 0));
    let mut bt = with(vec![lucky, viper], &[(71, f(2))], &[(95, f(2))]);
    bt.fighters[1].hp = 10;
    bt.act(1).unwrap();
    to_round(&mut bt, 2);
    assert!(!bt.fighters[1].alive());
}

#[test]
fn hold_line_does_nothing() {
    let line = bonus(82, Bonus::HoldLine, warrior(82, 30, 5));
    let bt = with(vec![line], &[(82, f(2)), (82, f(3))], &[(10, f(2))]);
    assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 25);
    assert_eq!(bt.physical_damage(2, 0, ActionKind::Melee), 25);
}

#[test]
fn neutralize_strips_the_targets_bonus() {
    let null = bonus(90, Bonus::Neutralize, UnitDef { hits: 300, ..warrior(90, 30, 0) });
    let chief = bonus(91, Bonus::Counterblow, UnitDef { hits: 300, ..warrior(91, 25, 0) });
    let mut bt = with(vec![null, chief], &[(10, f(3)), (90, f(2))], &[(91, f(2))]);
    assert_eq!(bt.act(2).unwrap().counter, Some(20), "a plain strike is answered: 25 − 5");
    assert_eq!(bt.act(2).unwrap().counter, None, "stripped before it can answer");
    assert!(bt.fighters[2].base.bonuses.is_empty());
}

#[test]
fn blood_thirst_gets_an_action_for_a_kill() {
    let fang = bonus(93, Bonus::BloodThrist, warrior(93, 30, 0));
    let mut bt = with(vec![fang], &[(93, f(2))], &[(10, f(2)), (10, f(3))]);
    bt.fighters[1].hp = 1;
    assert!(bt.act(1).unwrap().killed);
    assert_eq!((bt.active(), bt.actions_left()), (Some(0), 1));
    assert!(!bt.act(2).unwrap().killed);
    assert_ne!(bt.active(), Some(0));
}

// --- outcome, hero, XP -------------------------------------------------------------------------

#[test]
fn hero_survives_while_his_army_lives() {
    let mut bt = battle(&[(10, f(2)), (10, f(3))], &[(18, f(2))]);
    bt.fighters[0].hp = 0;
    assert_eq!(bt.outcome(), Outcome::Ongoing, "hero down, army fights on");
    bt.fighters[2].hp = 0;
    assert_eq!(bt.outcome(), Outcome::Victory);
    let res = bt.player_results();
    assert_eq!((res[0].hp, res[1].hp), (1, 50), "hero comes back badly wounded");
    let mut bt = battle(&[(10, f(2)), (10, f(3))], &[(18, f(2))]);
    bt.fighters[0].hp = 0;
    bt.fighters[1].hp = 0;
    assert_eq!(bt.outcome(), Outcome::Defeat, "defeat only when the whole army is dead");
    assert_eq!(bt.player_results()[0].hp, 0);
}

#[test]
fn deployment_is_kept_after_the_battle() {
    let c = content_with(vec![], Formation::WIDE);
    let squad = [Unit::new(&c, UnitId(10), f(2)), Unit::new(&c, UnitId(11), b(2))];
    let p: Vec<_> = squad.iter().enumerate().collect();
    let mut bt = Battle::new(c.clone(), &p, &[Unit::new(&c, UnitId(18), f(2))], Team::Player);
    assert_eq!(bt.active(), None);
    bt.move_card(f(2), b(2)).unwrap();
    assert_eq!((bt.fighters[0].slot, bt.fighters[1].slot), (b(2), f(2)));
    bt.move_card(f(2), f(5)).unwrap();
    assert_eq!(bt.move_card(f(0), f(1)), Err(ActionError::InvalidTarget));
    bt.begin();
    assert_eq!(bt.move_card(f(5), f(4)), Err(ActionError::NotDeploying));
    bt.fighters[0].slot = f(0); // pushed around during the fight
    let res = bt.player_results();
    assert_eq!((res[0].slot, res[1].slot), (b(2), f(5)));
}

/// The player: warrior 10 in front, shooter 11 behind; the enemy: four punching bags in
/// front. A bag: D = round((e⁰/1.17 + e⁰/1.07)·200) = 358, H = 558, A = 11,
/// T = round(3.2·558·12/200) = 107; the enemy side 428.
fn xp_battle() -> Battle {
    battle(&[(10, f(2)), (11, b(2))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(4))])
}

fn win(bt: &mut Battle) {
    for i in 2..bt.fighters.len() {
        bt.fighters[i].hp = 0;
    }
}

#[test]
fn xp_pool_and_shares_follow_the_original() {
    let mut bt = xp_battle();
    assert!(bt.xp_awards(Team::Player).is_empty(), "not over yet");
    assert_eq!(bt.start_of(Team::Enemy).strength, 428);
    assert_eq!(bt.start_of(Team::Player).count, 2);
    win(&mut bt);
    let xp = bt.xp_awards(Team::Player);
    assert_eq!(xp.iter().map(|a| (a.fighter, a.xp)).collect::<Vec<_>>(), vec![(0, 8), (1, 5)]);
    assert_eq!(bt.player_xp().iter().map(|a| a.xp).collect::<Vec<_>>(), vec![5, 3]);
    bt.set_xp_correction(250);
    assert_eq!(bt.player_xp().iter().map(|a| a.xp).collect::<Vec<_>>(), vec![12, 8]);
}

#[test]
fn xp_counts_activity_and_hit_points_lost() {
    let mut bt = xp_battle();
    turn_of(&mut bt, 0);
    let hp = bt.fighters[3].hp;
    bt.act(3).unwrap();
    let (a, t) = (&bt.fighters[0], &bt.fighters[3]);
    assert_eq!((a.useful, a.taken, a.actions), (1, 1, 0));
    assert_eq!(t.lost, hp - t.hp, "the target's loss is counted");
    turn_of(&mut bt, 1);
    bt.skip();
    assert_eq!((bt.fighters[1].useful, bt.fighters[1].taken, bt.fighters[1].actions), (0, 1, 0), "a wait spends the actions");
    win(&mut bt);
    assert_eq!(bt.xp_awards(Team::Player).iter().map(|a| a.xp).collect::<Vec<_>>(), vec![10, 5]);
    bt.fighters[0].lost = (bt.start_of(Team::Player).hp / 2) as i32;
    assert_eq!(bt.xp_awards(Team::Player).iter().map(|a| a.xp).collect::<Vec<_>>(), vec![5, 2]);
}

#[test]
fn the_dead_get_nothing_but_count_and_only_victory_pays_the_player() {
    let mut bt = battle(&[(10, f(2)), (11, b(2)), (11, b(3))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(4))]);
    bt.fighters[2].hp = 0;
    win(&mut bt);
    let xp = bt.xp_awards(Team::Player);
    assert_eq!(xp.iter().map(|a| (a.fighter, a.xp)).collect::<Vec<_>>(), vec![(0, 5), (1, 4)]);
    let mut lost = xp_battle();
    lost.fighters[0].hp = 0;
    lost.fighters[1].hp = 0;
    assert_eq!(lost.outcome(), Outcome::Defeat);
    assert!(lost.player_xp().is_empty(), "no XP without a victory");
    assert!(lost.ai_xp(Team::Player).is_empty(), "a wiped-out side has no strength left");
    assert_eq!(lost.ai_xp(Team::Enemy).len(), 4, "the AI survivors gain");
}

// --- row 30: the AI ---------------------------------------------------------------------------

#[test]
fn row30_ai_melee_scores_damage_by_the_answer_and_prefers_kills() {
    let mut bt = battle(&[(18, f(1)), (18, f(2)), (18, f(3))], &[(10, f(2))]);
    turn_of(&mut bt, 3);
    // All score 30 × (5 + 1) × 1: the tie goes to column 4, 3, 5 … (1-based) in that order.
    assert_eq!(bt.ai_choice(), Some((2, ActionKind::Melee)));
    bt.fighters[1].hp = 30;
    assert_eq!(bt.ai_choice(), Some((1, ActionKind::Melee)), "a kill");
}

#[test]
fn row30_the_kill_test_depends_on_the_ai_level() {
    let brute = acts(94, 2, warrior(94, 30, 5));
    let c = content_with(vec![brute], Formation::WIDE);
    for (improved, pick) in [(false, 2), (true, 0)] {
        let mut bt = prepared(&c, &[(18, f(1)), (18, f(2)), (18, f(3))], &[(94, f(2))], Team::Player);
        bt.set_improved_ai(improved);
        bt.begin();
        bt.fighters[0].hp = 60;
        turn_of(&mut bt, 3);
        assert_eq!(bt.ai_choice(), Some((pick, ActionKind::Melee)), "improved: {improved}");
    }
}

#[test]
fn row30_ai_mage_curses_first_then_strikes() {
    let lich = acts(96, 2, mage(96, 30, MagicSchool::Death, MagicDirection::ToEnemy));
    let mut bt = with(vec![lich], &[(10, f(2))], &[(96, b(2)), (18, f(2))]);
    turn_of(&mut bt, 1);
    let Some(Step::Act { hit, .. }) = bt.ai_step() else { panic!() };
    assert_eq!((hit.target, hit.kind), (0, ActionKind::Curse));
    let Some(Step::Act { hit, .. }) = bt.ai_step() else { panic!() };
    assert_eq!((hit.target, hit.kind), (0, ActionKind::Strike), "already cursed: strike");
}

#[test]
fn row30_ai_front_row_non_warrior_retreats_behind_the_healthiest() {
    let archer = acts(97, 2, shooter(97, 20));
    let mut bt = with(vec![archer], &[(10, f(0))], &[(97, f(2)), (10, f(3)), (18, f(1))]);
    turn_of(&mut bt, 1);
    assert_eq!(bt.ai_step(), Some(Step::Move { actor: 1, from: f(2), to: b(1) }), "behind the bag, 200 HP");
}

#[test]
fn row30_ai_warrior_uses_the_long_strike_or_steps_forward() {
    let mut bt = battle(&[(18, f(1))], &[(10, f(4))]);
    turn_of(&mut bt, 1);
    assert_eq!(bt.ai_choice(), Some((0, ActionKind::LongStrike)));
    let mut bt = battle(&[(18, f(2))], &[(18, f(2)), (10, b(3))]);
    turn_of(&mut bt, 2);
    assert_eq!(bt.ai_step(), Some(Step::Move { actor: 2, from: b(3), to: f(3) }));
}

#[test]
fn row30_ai_leaves_the_reserve_and_never_enters_it() {
    let c = content_with(vec![], Formation::VANILLA);
    let mut bt = battle_in(&c, &[(10, f(1))], &[(18, f(1)), (11, r(1)), (10, r(3))]);
    turn_of(&mut bt, 2);
    assert_eq!(bt.ai_step(), Some(Step::Move { actor: 2, from: r(1), to: b(1) }), "a shooter to the nearest back cell");
    turn_of(&mut bt, 3);
    let Some(Step::Move { to, .. }) = bt.ai_step() else { panic!() };
    assert_eq!(to.row, Front, "a warrior to the front row");
}

#[test]
fn auto_battles_terminate_deterministically_and_never_use_the_reserve() {
    let army = [(10, f(1)), (10, f(2)), (11, b(1)), (12, b(2)), (13, b(3)), (14, b(4)), (11, r(2)), (10, r(3))];
    for formation in [Formation::WIDE, Formation::VANILLA] {
        let c = content_with(vec![], formation);
        let mut logs = Vec::new();
        for _ in 0..2 {
            let mut bt = battle_in(&c, &army, &army);
            let mut steps = 0;
            while bt.outcome() == Outcome::Ongoing {
                let step = bt.ai_step().expect("an active fighter");
                if let Step::Move { from, to, .. } = step {
                    assert!(to.row != Reserve || from.row == Reserve, "into the reserve");
                }
                steps += 1;
                assert!(steps < 5000);
            }
            logs.push(bt.log.clone());
        }
        assert_eq!(logs[0], logs[1]);
    }
}

#[test]
fn real_armies_auto_battle_terminates() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    for formation in [Formation::WIDE, Formation::VANILLA] {
        let mut c = Content::from_dt(&dt);
        c.formation = formation;
        let c = Arc::new(c);
        let ids: Vec<UnitId> = c.unit_ids().collect();
        let mut outcomes = [0; 3];
        for (n, chunk) in ids.chunks(8).enumerate() {
            let army = |offset: usize| -> Vec<Unit> {
                let mut taken = Vec::new();
                (0..10)
                    .map(|i| {
                        let id = chunk[(i + offset) % chunk.len()];
                        let s = Stats::of_level(&c, id, 1);
                        let slot = c.formation.free_slot(&taken, s.preferred_row()).unwrap();
                        taken.push(slot);
                        Unit::new(&c, id, slot)
                    })
                    .collect()
            };
            let (p, e) = (army(0), army(n % 3 + 1));
            let squad: Vec<_> = p.iter().enumerate().collect();
            let mut bt = Battle::new(c.clone(), &squad, &e, Team::Enemy);
            if n % 2 == 1 {
                bt.set_simulation();
            }
            bt.begin();
            let mut steps = 0;
            while bt.outcome() == Outcome::Ongoing {
                if let Some(Step::Move { from, to, .. }) = bt.ai_step() {
                    assert!(to.row != Reserve || from.row == Reserve, "battle {n}: the AI never enters the reserve");
                }
                steps += 1;
                assert!(steps < 20_000, "battle {n} never ends");
            }
            outcomes[bt.outcome() as usize] += 1;
            assert!(bt.round <= 25);
            let pool = experience::battle_pool(bt.start_of(Team::Enemy).strength, bt.start_of(Team::Player).hp, 0).max(1);
            for a in bt.xp_awards(Team::Player) {
                assert!(a.xp >= 1 && a.xp as i64 <= pool, "battle {n}: share {} of pool {pool}", a.xp);
            }
            let gains = bt.player_xp();
            assert_eq!(gains.is_empty(), bt.outcome() != Outcome::Victory);
            assert!(gains.iter().all(|a| (0..=experience::MAX_BATTLE_XP).contains(&a.xp)));
        }
        assert_eq!(outcomes[0], 0);
        assert!(outcomes.iter().sum::<i32>() >= 12);
    }

}

/// Quick battle: a whole battle at once, the AI on both sides.
mod quick_battle {
    use super::*;

    /// Unit ids and cells of one side.
    type Side = Vec<(u32, Slot)>;

    fn armies() -> (Side, Side) {
        (vec![(1, f(1)), (10, f(2)), (11, b(1)), (13, b(2)), (10, r(0))], vec![(10, f(1)), (10, f(3)), (12, b(1)), (14, b(2)), (18, f(2))])
    }

    #[test]
    fn it_reaches_an_outcome_from_the_deploy_screen() {
        let c = content_with(vec![], Formation::WIDE);
        let (p, e) = armies();
        let mut bt = prepared(&c, &p, &e, Team::Enemy);
        assert!(bt.is_deploying());
        let outcome = bt.auto_play_to_end();
        assert!(!bt.is_deploying());
        assert_ne!(outcome, Outcome::Ongoing);
        assert_eq!(outcome, bt.outcome());
        assert!(bt.end_reason().is_some());
        assert!(bt.round <= c.options.battle_end_turn as u32);
        assert!(bt.auto_play_to_end() == outcome, "a finished battle stays as it is");
    }

    #[test]
    fn it_ends_exactly_as_the_same_battle_played_step_by_step() {
        let c = content_with(vec![], Formation::WIDE);
        let (p, e) = armies();
        let mut quick = prepared(&c, &p, &e, Team::Player);
        quick.auto_play_to_end();
        let mut played = prepared(&c, &p, &e, Team::Player);
        played.begin();
        while played.outcome() == Outcome::Ongoing {
            played.ai_step();
        }
        assert_eq!(quick.outcome(), played.outcome());
        assert_eq!(quick.end_reason(), played.end_reason());
        assert_eq!(quick.round, played.round);
        assert_eq!(quick.log, played.log);
        let hp = |b: &Battle| b.fighters.iter().map(|f| f.hp).collect::<Vec<_>>();
        assert_eq!(hp(&quick), hp(&played));
        assert_eq!(quick.player_xp(), played.player_xp());
        assert_eq!(quick.player_results(), played.player_results());
        assert_eq!(quick.surrender_mana(Team::Player), played.surrender_mana(Team::Player));
    }

    #[test]
    fn the_players_units_follow_the_same_rules() {
        let c = content_with(vec![], Formation::WIDE);
        let (p, e) = armies();
        let mut bt = prepared(&c, &p, &e, Team::Player);
        bt.set_improved_ai(false);
        bt.auto_play_to_end();
        assert!(bt.interactive && bt.ai_level == 1, "never the off-screen simulation's shortcuts");
        // Step by step: no unit of either side steps into the reserve, and a unit leaves
        // it at most once a turn.
        let mut bt = prepared(&c, &p, &e, Team::Player);
        bt.begin();
        let mut left_reserve: Vec<(u32, usize)> = Vec::new();
        while bt.outcome() == Outcome::Ongoing {
            let round = bt.round;
            if let Some(Step::Move { actor, from, to }) = bt.ai_step() {
                assert!(to.row != Reserve || from.row == Reserve, "nobody enters the reserve");
                if from.row == Reserve && to.row != Reserve {
                    assert!(!left_reserve.contains(&(round, actor)), "one reserve move a turn");
                    left_reserve.push((round, actor));
                }
            }
        }
    }

    #[test]
    fn a_lone_hero_against_a_strong_army_loses_and_gains_nothing() {
        let c = content_with(vec![], Formation::WIDE);
        let mut bt = prepared(&c, &[(18, f(1))], &[(10, f(1)), (10, f(2)), (11, b(1)), (12, b(2))], Team::Enemy);
        assert_eq!(bt.auto_play_to_end(), Outcome::Defeat);
        assert!(bt.player_xp().is_empty(), "no experience without a victory");
    }
}

/// The gameplay video's fort battle ("Форт в Трясине", РК3, 09:49): the garrison's starting
/// strength gives a pool of 75, a share of 25 for a unit that attacked all battle, and the
/// cuirassier, the sorceress and the hero gained "+25", "+24" and "+26". Razdor pays shares
/// at that rate (`PLAYER_XP_MODIFICATOR`), not the Community Update's halved one.
#[test]
fn real_fort_battle_pays_the_videos_xp() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    let c = Arc::new(Content::from_dt(&dt));
    assert_eq!(c.options.hero_experience_modificator, 100);
    let map = dt.maps.iter().find(|m| m.name.starts_with("РК3")).expect("РК3").load().expect("loads");
    let mut g = crate::rules::game::Game::from_scenario(c.clone(), &map, crate::rules::content::HeroClass::Archmage);
    let l = g.world.locations.iter().position(|l| l.name == "Форт в Трясине").expect("the fort");
    g.foe = Some(crate::rules::game::Foe::Garrison(l));
    let mut b = g.start_battle();
    b.begin();
    let e0 = b.strength_now(Team::Enemy);
    let pool = crate::rules::experience::battle_pool(e0, 1, 0);
    assert_eq!((e0, pool), (1516, 75));
    // Three units in the video's army; one that attacked with every action.
    let share = crate::rules::experience::share(pool, 3, Front, 1, 1, 0);
    assert_eq!(share, 25);
    // With a garrison's correction of 100 and "impossible difficulty" (F 100): the video's +25.
    assert_eq!(crate::rules::experience::player_gain(share, c.options.hero_experience_modificator, 100, 100), 25);
}

#[test]
fn an_enemys_worn_items_are_known_to_the_battle() {
    let mut c = content(units(), vec![item(113, ArtefactType::Amulet)]);
    c.formation = Formation::WIDE;
    let c = Arc::new(c);
    let hero = Unit::new(&c, UnitId(10), f(2));
    let mut shade = Unit::new(&c, UnitId(18), f(2));
    shade.items[0] = Some(ItemId(113));
    let bt = Battle::new(c.clone(), &[(0, &hero)], &[shade], Team::Player);
    assert_eq!(bt.fighters[1].items[0], Some(ItemId(113)), "the panel shows what the enemy wears");
}

#[test]
fn the_cards_show_the_actions_left_this_turn() {
    let quick = UnitDef { manevres: 2, ..warrior(10, 30, 5) };
    let c = content_with(vec![quick], Formation::WIDE);
    let mut bt = prepared(&c, &[(10, f(0))], &[(18, f(0))], Team::Player);
    assert_eq!(bt.shown_stats(0)[Stat::Manevres], 2, "deploying: the plain stat");
    bt.begin();
    turn_of(&mut bt, 0);
    assert_eq!(bt.shown_stats(0)[Stat::Manevres], 2);
    bt.pass();
    assert_eq!(bt.shown_stats(0)[Stat::Manevres], 1, "one action spent");
    bt.fighters[0].actions += 2;
    assert_eq!(bt.shown_stats(0)[Stat::Manevres], 3, "a haste shows as more");
}

#[test]
fn a_piercing_blow_on_the_invulnerable_is_one_hit() {
    let piercer = bonus(40, Bonus::ArmorIgnore, UnitDef { initiative: 30, ..warrior(40, 60, 0) });
    let stone = bonus(31, Bonus::Unvulnerabe, UnitDef { hits: 50, ..warrior(31, 1, 30) });
    let ghost = bonus(33, Bonus::Ghost, UnitDef { hits: 50, ..warrior(33, 1, 30) });
    for target in [31, 33] {
        let mut bt = with(vec![piercer.clone(), stone.clone(), ghost.clone()], &[(40, f(2))], &[(target, f(2))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 1, "{target}");
        turn_of(&mut bt, 0);
        let hit = bt.act(1).unwrap();
        assert_eq!((hit.amount, bt.fighters[1].hp), (1, 49), "{target}: {hit:?}");
    }
    // The Wrath and Anger of God add nothing to it either.
    for god in [Bonus::GodAnger, Bonus::GodStrike] {
        let smiter = bonus(40, god.clone(), UnitDef { initiative: 30, ..warrior(40, 60, 0) });
        let bt = with(vec![smiter, stone.clone(), ghost.clone()], &[(40, f(2))], &[(31, f(2)), (33, f(3))]);
        assert_eq!((bt.physical_damage(0, 1, ActionKind::Melee), bt.physical_damage(0, 2, ActionKind::Melee)), (1, 1), "{god:?}");
    }
}
