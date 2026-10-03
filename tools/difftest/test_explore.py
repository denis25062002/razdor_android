"""Checks of the explorer's JSON repair and of the known-findings matcher (no game needed).

    ~/.local/opt/re-venv/bin/python -m unittest tools.difftest.test_explore
"""

import unittest

from . import explore, known


class Repair(unittest.TestCase):
    def test_clean(self):
        acts, st = explore.repair('{"actions":[{"op":"click_map","x":3,"y":"4"},{"op":"wait","hours":4}]}')
        self.assertEqual(acts, [{"op": "click_map", "x": 3, "y": 4}, {"op": "wait", "hours": 4}])
        self.assertEqual(st["repaired"], 0)

    def test_fences_trailing_comma_aliases(self):
        text = 'Sure!\n```json\n{"actions":[{"op":"Accept"},{"op":"move","cell":[5,6]},]}\n```'
        acts, st = explore.repair(text)
        self.assertEqual(acts, [{"op": "answer", "yes": True}, {"op": "click_map", "x": 5, "y": 6}])
        self.assertEqual(st["repaired"], 1)

    def test_dropped(self):
        acts, st = explore.repair('[{"op":"battle_auto"},{"op":"fly"},{"op":"battle_act","row":1,"col":9},'
                                  '{"op":"wait","hours":3},{"op":"answer","yes":"no"}]')
        self.assertEqual(acts, [{"op": "wait", "hours": 4}, {"op": "answer", "yes": False}])
        self.assertEqual(st["dropped"], 3)

    def test_services(self):
        acts, st = explore.repair('{"actions":[{"op":"purchase","slot":"2"},{"op":"cast","slot":0,"army":7},'
                                  '{"op":"equip","item":1},{"op":"cure","unit":3},{"op":"learn"}]}')
        self.assertEqual(acts, [{"op": "buy", "slot": 2}, {"op": "cast", "slot": 0, "army": 7},
                                {"op": "equip", "slot": 1, "unit": 0}, {"op": "heal", "unit": 3}])
        self.assertEqual(st["dropped"], 1)

    def test_valid_ops(self):
        look = {"goods": [{}], "book": [{}], "services": [{"op": "heal"}], "spells": [{"known": True}]}
        self.assertEqual(explore.valid_ops("building", look),
                         ("ok", "click_map", "wait", "cast", "buy", "heal"))
        self.assertEqual(explore.valid_ops("offer", look), ("answer",))

    def test_battle_start(self):
        screens = ["map", "map", "battle", "battle", "battle", "ended"]
        self.assertEqual(explore.battle_start(screens, 4), 2)

    def test_prompt_cap(self):
        text = "\n".join(f"line {i} " + "x" * 200 for i in range(400))
        out = explore.cap_prompt(text, 5000)
        self.assertLessEqual(len(out), 5000)
        self.assertTrue(out.startswith("line 0 ") and out.rstrip().endswith("x"))
        self.assertIn("line 399 ", out)
        self.assertEqual(explore.cap_prompt("short"), "short")

    def test_garbage(self):
        acts, st = explore.repair("I would walk north.")
        self.assertEqual(acts, [])
        self.assertEqual(st["unparsed"], 1)


def ctx(actions, orun, rrun, buildings=None):
    return known.Context(actions, buildings or {}, [], [], orun, rrun)


class Matcher(unittest.TestCase):
    def test_patroller_draws(self):
        c = ctx([{"op": "wait", "hours": 1}],
                [{"step": 0, "draws": [[5, 0, "0x1", "x"], [3000, 0, "0x4ad933", "patroller idle offset"]]}],
                [{"step": 0, "draws": [[5, 0, "src/rules/x.rs:1"]]}])
        per = known.classify([{"step": 0, "diffs": [["rng", 1, 2]]}], c)
        self.assertEqual(per[0][0]["class"], "known:1")

    def test_ai_order_taints_armies(self):
        w = lambda a, b, site: [[a if k % 2 == 0 else b, 0, site] for k in range(8)]
        o9, o1 = [d[:2] + ["0x4a2594", "AI wander points"] for d in w(40, 40, "")], \
            [d[:2] + ["0x4a2594", "AI wander points"] for d in w(17, 14, "")]
        c = ctx([{"op": "wait", "hours": 4}] * 2,
                [{"step": 0, "draws": [[7, 0, "0x1", "x"]] + o9 + o1}, {"step": 1, "draws": []}],
                [{"step": 0, "draws": [[7, 0, "x"]] + w(17, 14, "src/rules/ai.rs:1668") + w(40, 40, "src/rules/ai.rs:1668")},
                 {"step": 1, "draws": []}])
        per = known.classify([{"step": 0, "diffs": [["armies[id 1].x", 4, 9], ["rng", 1, 2]]},
                              {"step": 1, "diffs": [["armies[id 1].y", 4, 9], ["hero.x", 1, 2]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["known:5", "downstream:5"])
        self.assertEqual([e["class"] for e in per[1]], ["downstream:5", "new"])

    def test_a_wander_box_of_its_own_is_new(self):
        # Random(50)/Random(50) in the original where Razdor draws Random(40)/Random(40) and
        # neither side draws the other's ranges: an area difference, not an order one.
        w = lambda a, b, site: [[a if k % 2 == 0 else b, 0, site, "AI wander points"] for k in range(8)]
        c = ctx([{"op": "wait", "hours": 4}],
                [{"step": 0, "draws": w(50, 50, "0x4a2624")}],
                [{"step": 0, "draws": [d[:3] for d in w(40, 40, "src/rules/ai.rs:1668")]}])
        per = known.classify([{"step": 0, "diffs": [["rng", 1, 2]]}], c)
        self.assertEqual(per[0][0]["class"], "new")

    def test_a_hire_in_another_order_is_ai_order(self):
        # C1003-234059: the original's army 2 hires (Random(54)) before army 26's wander
        # points in the same frame; Razdor after them, by their exact times.
        w = [[11, 0, "0x4a2594", "AI wander points"], [12, 0, "0x4a25d8", "AI wander points"]] * 4
        c = ctx([{"op": "click_map", "x": 6, "y": 6}],
                [{"step": 0, "draws": [[54, 0, "0x4a6b74", "AI hire XP"]] + w}],
                [{"step": 0, "draws": [d[:2] + ["src/rules/ai.rs:1668"] for d in w] + [[54, 0, "src/rules/ai.rs:2385"]]}])
        per = known.classify([{"step": 0, "diffs": [["rng", 1, 2]]}], c)
        self.assertEqual(per[0][0]["class"], "known:5")

    def test_a_midnight_restock_against_wander_points_is_ai_order(self):
        # rk1-day1 step 16: Razdor's midnight restocks market 5 where the original first
        # draws army 9's wander points (its midnight runs at the end of the frame).
        c = ctx([{"op": "wait", "hours": 4}],
                [{"step": 0, "draws": [[1, 0, "0x1", "barracks"], [40, 0, "0x4a2594", "AI wander points"]]}],
                [{"step": 0, "draws": [[1, 0, "src/rules/economy.rs:447"], [5, 0, "src/rules/economy.rs:567"]]}])
        per = known.classify([{"step": 0, "diffs": [["rng", 1, 2]]}], c)
        self.assertEqual(per[0][0]["class"], "known:5")
        # A midnight against a battle draw is not.
        c = ctx([{"op": "wait", "hours": 4}],
                [{"step": 0, "draws": [[5, 0, "0x4be2a4", "market restock"]]}],
                [{"step": 0, "draws": [[100, 0, "src/rules/battle.rs:10"]]}])
        per = known.classify([{"step": 0, "diffs": [["rng", 1, 2]]}], c)
        self.assertEqual(per[0][0]["class"], "new")

    def test_formation_taints_all(self):
        c = ctx([{"op": "click_map", "x": 1, "y": 1}], [{"step": 0}], [{"step": 0}])
        per = known.classify([{"step": 0, "diffs": [["battle.actor", 1, 2], ["battle.sides[0][0].hp", 80, 48],
                                                          ["battle.sides[0][0].row", 3, 1]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["known:3", "downstream:3", "downstream:3"])

    def test_desync_after_known(self):
        c = ctx([{"op": "click_map", "x": 1, "y": 1}, {"op": "click_map", "x": 5, "y": 5}],
                [{"step": 0, "draws": [[3000, 0, "0x4ad933", "patroller idle offset"]]},
                 {"step": 1, "note": "skipped: on screen event"}],
                [{"step": 0, "draws": []}, {"step": 1, "notes": []}])
        per = known.classify([{"step": 0, "diffs": [["rng", 1, 2]]}, {"step": 1, "diffs": [["hero.x", 1, 2]]}], c)
        self.assertEqual([e["class"] for e in per[1]], ["downstream:1", "downstream:1"])

    def test_one_cell_off_is_noise(self):
        c = known.Context([{"op": "wait", "hours": 1}], {},
                          [{"step": 0, "armies": [{"id": 9, "x": 5, "y": 24}]}],
                          [{"step": 0, "armies": [{"id": 9, "x": 5, "y": 25}]}],
                          [{"step": 0, "draws": [[3000, 0, "0x4ad933", "patroller idle offset"]]}],
                          [{"step": 0, "draws": []}])
        per = known.classify([{"step": 0, "diffs": [["armies[id 9].y", 24, 25], ["rng", 1, 2]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["known:1", "noise"])

    def test_event_results_at_ok(self):
        c = known.Context([{"op": "click_map", "x": 1, "y": 1}], {},
                          [{"step": 0, "events_done": [1, 161]}], [],
                          [{"step": 0, "meta": {"screen": "event", "dialog_event": 160, "event_count": 269}}],
                          [{"step": 0}])
        per = known.classify([{"step": 0, "diffs": [["hero.gold", 490, 500], ["clock", 1, 2]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["new", "timing"])

    def test_events_queued_behind_the_window(self):
        # C1003-174531: event 4 on screen, event 5 fired with it in Razdor, in the original
        # when event 4's window closes.
        c = known.Context([{"op": "click_map", "x": 1, "y": 1}], {},
                          [{"step": 0, "events_done": [1, 2, 3, 4, 5]}], [],
                          [{"step": 0, "meta": {"screen": "event", "dialog_event": 3, "event_count": 22}}],
                          [{"step": 0}])
        per = known.classify([{"step": 0, "diffs": [["events_done", [1, 2, 3, 4, 5], [1, 2, 3, 4]]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["timing"])

    def test_a_question_on_screen_is_not_done_in_razdor_yet(self):
        # C1004-042357: event 6 is a question in both; the differ counts the original's shown
        # event as done, Razdor counts it once answered.
        c = known.Context([{"op": "click_map", "x": 20, "y": 28}], {}, [], [],
                          [{"step": 0, "meta": {"screen": "event", "dialog_event": 5, "event_count": 22}}],
                          [{"step": 0}])
        per = known.classify([{"step": 0, "diffs": [["events_done", [1, 5, 9], [1, 5, 6, 9]]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["timing"])

    def test_tribute_while_the_village_window_is_open(self):
        c = known.Context([{"op": "click_map", "x": 96, "y": 18}], {}, [], [],
                          [{"step": 0, "meta": {"screen": "village"}}], [{"step": 0}])
        per = known.classify([{"step": 0, "diffs": [["hero.gold", 1050, 1000], ["buildings[id 13].mana", 0, 40],
                                                          ["hero.x", 1, 2]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["timing", "timing", "new"])

    def test_an_xp_difference_is_new(self):
        # FINDINGS §6 is fixed: Razdor pays the install's rate, so XP must agree again.
        c = ctx([{"op": "battle_act"}], [{"step": 0}], [{"step": 0}])
        per = known.classify([{"step": 0, "diffs": [["hero.units[0].xp", 33, 16]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["new"])

    def test_paths(self):
        st = {"armies": [{"id": 3, "x": 7}], "battle": {"sides": [[], [{"hp": 5}]]}}
        self.assertEqual(explore.get_path(st, "armies[id 3].x"), 7)
        self.assertEqual(explore.get_path(st, "battle.sides[1][0].hp"), 5)
        self.assertEqual(explore.get_path(st, "battle.sides[0].len"), 0)
        self.assertEqual(known.norm_path("armies[id 3].units[2].hp"), "armies[].units[].hp")


if __name__ == "__main__":
    unittest.main()
