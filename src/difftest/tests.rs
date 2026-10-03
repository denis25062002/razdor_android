use super::*;
use crate::dt::dtm::Archetype;
use crate::rules::rng::{Rng, WORLD_MUSIC_DRAW};

fn demo_walk() -> Vec<Action> {
    parse_actions(
        r#"
        # the demo: a walk, an hour and four
        {"op":"new_game","map":"demo","hero":1}
        {"op":"click_map","x":TX,"y":TY}
        {"op":"snapshot"}
        {"op":"wait","hours":1}
        {"op":"wait","hours":4}
        "#
        .replace("TX", &target().0.to_string())
        .replace("TY", &target().1.to_string())
        .as_str(),
    )
    .unwrap()
}

/// A cell three steps east of the demo's start that a click can walk to.
fn target() -> (i32, i32) {
    let g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
    let (x, y) = g.tile();
    (1..=6).flat_map(|d| [(x + d, y), (x - d, y), (x, y + d), (x, y - d)]).find(|&t| !g.route_to(t).is_empty() && g.world.location_at(t).is_none()).expect("a free cell near the start")
}

#[test]
fn every_action_of_v1_parses() {
    let text = r#"{"op":"new_game","map":"РК1.DTm","hero":2}
{"op":"click_map","x":3,"y":4}
{"op":"wait","hours":4}
{"op":"key","key":"Escape"}
{"op":"answer","yes":false}
{"op":"ok"}
{"op":"battle_auto"}
{"op":"snapshot"}"#;
    let a = parse_actions(text).unwrap();
    assert_eq!(a.len(), 8);
    assert_eq!(a[0], Action::NewGame { map: "РК1.DTm".into(), hero: 2 });
    assert_eq!(a[1], Action::ClickMap { x: 3, y: 4 });
    assert_eq!(a[4], Action::Answer { yes: false });
    assert_eq!(a[6], Action::BattleAuto);
    assert!(parse_actions(r#"{"op":"fly"}"#).is_err());
}

#[test]
fn the_demo_replays_a_walk_and_waits() {
    let actions = demo_walk();
    let (states, notes) = replay(Source::Demo, &actions).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(states.len(), actions.len());
    assert_eq!(states.iter().map(|s| s.step).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);

    // The start: the demo's clock, the generator after the map load's draws.
    let fresh = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
    let s0 = &states[0];
    assert_eq!(s0.map, "demo");
    assert_eq!(s0.clock, fresh.clock.total_minutes() as u64);
    assert_eq!(s0.rng, fresh.rng.state());
    assert_eq!((s0.hero.x, s0.hero.y), fresh.tile());
    assert_eq!(s0.hero.gold, fresh.gold);
    assert_eq!(s0.hero.units.len(), 1);
    assert_eq!(s0.hero.units[0].kind, 1, "the knight");
    assert_eq!(s0.hero.units[0].hp, fresh.hero().hp, "unhurt");

    // The click walks there, with the route's game time.
    let t = target();
    let minutes = fresh.travel_minutes(&fresh.route_to(t)) as u64;
    let s1 = &states[1];
    assert_eq!((s1.hero.x, s1.hero.y), t);
    assert_eq!(s1.clock, s0.clock + minutes);
    assert_eq!(states[2], State { step: 2, ..s1.clone() }, "a snapshot changes nothing");

    // The waits: an hour, then four (the demo starts at 08:00: no noon report in between).
    assert_eq!(states[3].clock, s1.clock + 60);
    assert_eq!(states[4].clock, s1.clock + 300);

    // The same actions give the same states.
    assert_eq!(replay(Source::Demo, &actions).unwrap().0, states);

    // One line of JSON per step, the schema's fields.
    let v: serde_json::Value = serde_json::to_value(&states[4]).unwrap();
    for k in ["step", "map", "clock", "rng", "hero", "armies", "buildings", "events_done"] {
        assert!(v.get(k).is_some(), "{k}");
    }
    assert!(v["hero"]["units"][0].get("type").is_some());
}

#[test]
fn input_that_does_not_apply_is_noted() {
    let actions = parse_actions(
        r#"{"op":"new_game","map":"demo","hero":3}
{"op":"ok"}
{"op":"battle_auto"}
{"op":"answer","yes":true}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Demo, &actions).unwrap();
    assert_eq!(states.len(), 4);
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert!(states.windows(2).all(|w| w[0].rng == w[1].rng && w[0].clock == w[1].clock));
    assert!(replay(Source::Demo, &parse_actions(r#"{"op":"wait","hours":1}"#).unwrap()).is_err(), "no game yet");
}

fn install() -> Option<DtInstall> {
    let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
    Some(DtInstall::load(Path::new(&dir)).expect("install loads"))
}

/// The LCG's step back: the multiplier's inverse mod 2³².
fn unstep(s: u32) -> u32 {
    let mut inv: u32 = 1;
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub(214_013u32.wrapping_mul(inv)));
    }
    assert_eq!(inv.wrapping_mul(214_013), 1);
    s.wrapping_sub(2_531_011).wrapping_mul(inv)
}

/// РК1 just loaded: the hero, his gold and the armies are where the map file puts them, and
/// the generator is where the load sequence leaves it (engine.md §3.2): 1, the market
/// draws, the world music's `Random(90000)` last; then the chords of the windows open.
#[test]
fn rk1_after_load_matches_the_map_file() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(r#"{"op":"new_game","map":"РК1","hero":1}"#).unwrap();
    let mut r = Runner::new(Source::Install(&dt));
    r.apply(&actions[0]).unwrap();
    let s0 = r.state(0).unwrap();
    println!("{}", serde_json::to_string(&s0).unwrap());
    let m = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap();
    let s = m.load().unwrap();
    assert_eq!(s0.map, format!("{}.DTm", m.name));

    let p = s.header.hero(Archetype::Knight);
    assert_eq!((s0.hero.x, s0.hero.y), (p.x as i32, p.y as i32));
    assert_eq!(s0.hero.gold, p.gold as u16 as i16 as i32);
    assert_eq!(s0.hero.mana, p.mana as u16 as i16 as i32);
    assert_eq!(s0.clock, s.header.start_time as u64 + 1, "the start minute + 1");

    assert_eq!(s0.armies.iter().map(|a| a.id).collect::<Vec<_>>(), s.armies.iter().map(|a| a.id as i32).collect::<Vec<_>>());
    for (a, f) in s0.armies.iter().zip(&s.armies) {
        assert!(a.alive);
        assert_eq!(a.active, Some(f.is_active()), "army {}", a.id);
        if f.is_active() {
            assert_eq!((a.x, a.y), (Some(f.x as i32), Some(f.y as i32)), "army {}", a.id);
        }
    }
    assert!(s0.armies.iter().flat_map(|a| a.units.iter().flatten()).all(|u| u.hp > 0), "every unit unhurt");
    assert!(s0.hero.units.iter().all(|u| u.hp > 0 && u.level >= 0));
    // The opening events fired, nothing else.
    let e = r.game().unwrap().script().unwrap();
    assert!(s0.events_done.iter().all(|&id| e.times_fired(id as u16) > 0));
    assert_eq!(s0.buildings.iter().map(|b| b.id).collect::<Vec<_>>(), (1..=s.buildings.len() as i32).collect::<Vec<_>>());

    // The generator: a fresh load without the interface ends on the music's draw, made
    // after the market draws from 1.
    let fresh = Game::from_scenario(Arc::new(Content::from_dt(&dt)), &s, HeroClass::Knight);
    let after = fresh.rng.state();
    let mut music = Rng::new(unstep(after));
    assert_eq!(fresh.music_wait, Some(90_000 + music.random(WORLD_MUSIC_DRAW) as u32));
    let before_music = unstep(after);
    let mut k = 0;
    let mut x = Rng::new(1);
    while x.state() != before_music {
        x.random(1);
        k += 1;
        assert!(k < 1_000_000, "the state before the music is not reached from 1");
    }
    println!("РК1: {k} market draws from 1, then the music; {} chord draws", r.ui_draws);
    let mut ui = Rng::new(after);
    for _ in 0..r.ui_draws {
        ui.random(3);
    }
    assert_eq!(s0.rng, ui.state());
}

#[test]
fn battle_actions_parse() {
    let a = parse_actions(
        r#"{"op":"battle_act","side":2,"row":1,"col":4}
{"op":"battle_pass"}"#,
    )
    .unwrap();
    assert_eq!(a, vec![Action::BattleAct { side: 2, row: 1, col: 4 }, Action::BattlePass]);
}

/// The draw trace: each step's draws, stepped from their first state, end on the state the
/// step leaves; the step-local mode starts each step from the given states.
#[test]
fn draws_are_traced_and_the_generator_can_be_synced() {
    let actions = demo_walk();
    let r = replay_traced(Source::Demo, &actions, None).unwrap();
    assert_eq!(r.draws.len(), actions.len());
    assert!(!r.draws[0].is_empty(), "the load draws (music)");
    for (i, d) in r.draws.iter().enumerate() {
        if let Some(first) = d.first() {
            let mut g = Rng::new(first.before);
            for x in d {
                assert_eq!(g.state(), x.before);
                g.random(x.n);
            }
            assert_eq!(g.state(), r.states[i].rng, "step {i}");
        }
    }
    let wanted: Vec<u32> = (0..actions.len() as u32).map(|k| 1000 + k).collect();
    let synced = replay_traced(Source::Demo, &actions, Some(&wanted)).unwrap();
    for (i, d) in synced.draws.iter().enumerate().skip(1) {
        if let Some(first) = d.first() {
            assert_eq!(first.before, wanted[i - 1], "step {i} starts from the given state");
        } else {
            assert_eq!(synced.states[i].rng, wanted[i - 1], "step {i} draws nothing");
        }
    }
}

/// РК1, knight: the walk to the ruins north of the start ends in the garrison's fight; the
/// battle shows in the state with the original's cell numbers, a press on an enemy card
/// strikes it, the space key passes, and the levels read as the map file numbers them.
#[test]
fn rk1_battle_by_actions() {
    let Some(dt) = install() else { return };
    let head = r#"{"op":"new_game","map":"РК1","hero":1}
{"op":"ok"}
{"op":"click_map","x":40,"y":32}
{"op":"ok"}
{"op":"ok"}
{"op":"wait","hours":1}
{"op":"ok"}
{"op":"click_map","x":38,"y":28}
{"op":"ok"}
{"op":"click_map","x":36,"y":23}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":36,"y":23}"#;
    let mut actions = parse_actions(head).unwrap();
    let (states, _) = replay(Source::Install(&dt), &actions).unwrap();
    assert!(states[0].hero.units.iter().all(|u| u.level == 0), "level 0 as in the file");
    let b = states.last().unwrap().battle.clone().expect("the garrison's battle");
    assert_eq!(b.turn, 1);
    assert_eq!(b.sides[1].iter().map(|u| u.kind).collect::<Vec<_>>(), [66, 65, 59, 59]);
    assert!(b.sides.iter().flatten().all(|u| (1..=3).contains(&u.row) && (1..=6).contains(&u.col)));
    let actor = b.actor.expect("the player's turn");
    assert_eq!(actor[0], 1);
    let acting = |b: &BattleState| b.actor.and_then(|a| b.sides[0].iter().find(|u| [1, u.row, u.col] == a).cloned()).unwrap();
    let before = acting(&b).actions;

    // The space key: one action of the acting unit.
    actions.push(Action::BattlePass);
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert!(notes.iter().all(|n| !n.contains("battle")), "{notes:?}");
    let b1 = states.last().unwrap().battle.clone().unwrap();
    if b1.actor == b.actor {
        assert_eq!(acting(&b1).actions, before - 1);
    }

    // The novice has no action on an unhurt enemy: she passes her second action too.
    actions.push(Action::BattlePass);
    let (states, _) = replay(Source::Install(&dt), &actions).unwrap();
    let b1 = states.last().unwrap().battle.clone().unwrap();

    // Presses on the enemy's cards until one strikes: an enemy loses hit points.
    let hp = |b: &BattleState| b.sides[1].iter().map(|u| u.hp).sum::<i32>();
    let mut struck = false;
    for u in &b1.sides[1] {
        let mut a = actions.clone();
        a.push(Action::BattleAct { side: 2, row: u.row, col: u.col });
        let (states, notes) = replay(Source::Install(&dt), &a).unwrap();
        if notes.iter().any(|n| n.starts_with(&format!("step {}:", a.len() - 1))) {
            continue;
        }
        let after = states.last().unwrap();
        struck = after.battle.as_ref().is_none_or(|b2| hp(b2) < hp(&b1));
        break;
    }
    assert!(struck, "some enemy card takes a strike");
}

/// The original's generator after each step of `tools/difftest/rk1-day1.jsonl` (the diff
/// test's run `rk1-day1`, read from the running Discord Times).
const RK1_DAY1_ORIGINAL_RNG: [u32; 44] = [
    10044473, 10044473, 2785918235, 120733505, 120733505, 18883840, 18883840, 2739985274, 2739985274, 775978695, 325516910, 325516910, 108516897, 108516897, 1831002011, 1831002011, 172678701, 172678701, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517,
    2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2849548517, 2317907988, 2317907988, 3523995438, 3523995438, 2816094272, 2816094272, 2416142925, 2416142925,
];

/// `rk1-day1.jsonl` replayed step-locally (each step from the original's generator): the
/// steps whose generator ends as the original's.
fn rk1_day1_equal_steps(dt: &DtInstall) -> Vec<usize> {
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    assert_eq!(actions.len(), RK1_DAY1_ORIGINAL_RNG.len());
    let r = replay_traced(Source::Install(dt), &actions, Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    r.states.iter().enumerate().filter(|(i, s)| s.rng == RK1_DAY1_ORIGINAL_RNG[*i]).map(|(i, _)| i).collect()
}

/// FINDINGS.md §1: the stops of the first day (the wait of step 5, the walk of step 7, the
/// event of step 9, the wait of step 12) draw the idle offsets the original draws.
#[test]
fn rk1_day1_the_stops_draw_as_the_original() {
    let Some(dt) = install() else { return };
    let equal = rk1_day1_equal_steps(&dt);
    println!("rk1-day1, steps equal to the original's: {equal:?}");
    for step in [4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {
        assert!(equal.contains(&step), "step {step}: {equal:?}");
    }
}

/// FINDINGS.md §2: the village of step 2 is entered as event 17 opens; its offer rolls and
/// window chord come after the event's OK (step 3), as in the original.
#[test]
fn rk1_day1_the_village_waits_for_the_event_window() {
    let Some(dt) = install() else { return };
    let equal = rk1_day1_equal_steps(&dt);
    for step in [2, 3] {
        assert!(equal.contains(&step), "step {step}: {equal:?}");
    }
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let (states, _) = replay(Source::Install(&dt), &actions[..4]).unwrap();
    assert_eq!(states[2].hero.gold, 100, "no tribute under the event's window");
    assert_eq!(states[3].hero.gold, 140, "the village entered after it");
}

/// FINDINGS.md §3: the knight's army enters the ruins' battle (step 18) in the formation the
/// map load's auto-arrange gave it, as read in the original: knight and militia in front,
/// hunter and novice behind.
#[test]
fn rk1_day1_the_start_army_stands_as_auto_arranged() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions[..19], Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    let b = r.states[18].battle.clone().expect("the ruins' battle");
    let own: Vec<(i32, i32, i32)> = b.sides[0].iter().map(|u| (u.kind, u.row, u.col)).collect();
    assert_eq!(own, [(1, 1, 4), (4, 1, 3), (19, 2, 4), (26, 2, 3)]);
}

/// FINDINGS.md §4: the ruins' robber wears their Round shield, so the hunter's shot of
/// step 21 takes 8 HP off him (57 left), as in the original, not 12.
#[test]
fn rk1_day1_the_ruins_garrison_wears_their_goods() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(include_str!("../../tools/difftest/rk1-day1.jsonl")).unwrap();
    let r = replay_traced(Source::Install(&dt), &actions[..22], Some(&RK1_DAY1_ORIGINAL_RNG)).unwrap();
    let b = r.states[21].battle.clone().expect("the ruins' battle");
    assert_eq!((b.sides[1][0].kind, b.sides[1][0].hp), (66, 57));
    assert_eq!(r.states[0].buildings[7].goods, [51], "the building's goods words stay");
}

/// FINDINGS.md §5: in the waits of steps 14 and 16 the armies' arrivals come in the order of
/// their play times (army 9's mid-tick arrival draws before army 1's at the tick's end) and
/// the midnight's restock after the arrivals before it, so the generator ends each wait as
/// the original's.
#[test]
fn rk1_day1_arrivals_come_in_the_order_of_their_times() {
    let Some(dt) = install() else { return };
    let equal = rk1_day1_equal_steps(&dt);
    for step in [14, 15, 16, 17] {
        assert!(equal.contains(&step), "step {step}: {equal:?}");
    }
}

/// FINDINGS.md §8 (candidate C1003-173909): on ДС1 the walk to the village at (96,18) crosses
/// its cell (95,15); the village is taken on the way without a window and the walk goes on
/// (0x4ad94c), so he reaches the village after 150 minutes, as in the original, and enters it.
#[test]
fn ds1_a_village_crossed_on_the_way_does_not_stop_the_walk() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"ДС1-С чего все начиналось","hero":1}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":96,"y":18}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    let s = &states[5];
    assert_eq!((s.hero.x, s.hero.y, s.clock), (96, 18, 151), "{notes:?}");
}

/// FINDINGS.md §9 (candidate C1003-175950): Тихая пристань starts the hero on the water. The
/// map load prices his first step on LAND, before he is at sea (0x497c68), where water costs
/// 0: of the four shallow-water steps to (48,5) the first takes no time, so the walk takes 30
/// minutes, as in the original, not 40.
#[test]
fn quiet_harbour_the_first_step_at_sea_takes_no_time() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"Тихая пристань","hero":1}
{"op":"ok"}
{"op":"ok"}
{"op":"click_map","x":48,"y":5}"#,
    )
    .unwrap();
    let (states, notes) = replay(Source::Install(&dt), &actions).unwrap();
    assert_eq!((states[3].hero.x, states[3].hero.y), (48, 5), "{notes:?}");
    assert_eq!(states[3].clock - states[2].clock, 30);
}

/// FINDINGS.md §10-§13 (candidate C1003-174927): on Проклятое озеро the first four-hour wait
/// moves 28 AI armies through 509 draws. With the first step in place priced south of each
/// army (§10), the simulated battles counted in side strengths (§11) from the strengths of
/// the armies' last recount (§12) and the negative aggression's tenth only for a side that
/// lost no unit (§13), the wait ends as the original's: the generator and every army.
#[test]
fn cursed_lake_the_first_wait_moves_the_armies_as_the_original() {
    let Some(dt) = install() else { return };
    let actions = parse_actions(
        r#"{"op":"new_game","map":"Проклятое озеро","hero":1}
{"op":"ok"}
{"op":"wait","hours":4}"#,
    )
    .unwrap();
    let (states, _) = replay(Source::Install(&dt), &actions).unwrap();
    let s = &states[2];
    assert_eq!(s.rng, 996_532_122);
    let at = |id: i32| s.armies.iter().find(|a| a.id == id).map(|a| (a.x.unwrap(), a.y.unwrap()));
    assert_eq!([2, 9, 10, 13].map(at), [Some((10, 45)), Some((16, 59)), Some((74, 39)), Some((9, 68))]);
}
