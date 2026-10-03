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
        c = ctx([{"op": "wait", "hours": 4}] * 2,
                [{"step": 0, "draws": [[7, 0, "0x1", "x"], [40, 0, "0x4a2594", "AI wander points"]]},
                 {"step": 1, "draws": []}],
                [{"step": 0, "draws": [[7, 0, "x"], [17, 0, "src/rules/ai.rs:1465"]]}, {"step": 1, "draws": []}])
        per = known.classify([{"step": 0, "diffs": [["armies[id 1].x", 4, 9], ["rng", 1, 2]]},
                              {"step": 1, "diffs": [["armies[id 1].y", 4, 9], ["hero.x", 1, 2]]}], c)
        self.assertEqual([e["class"] for e in per[0]], ["known:5", "downstream:5"])
        self.assertEqual([e["class"] for e in per[1]], ["downstream:5", "new"])

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
