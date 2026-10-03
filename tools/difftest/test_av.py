"""Checks of the sound and animation differ that need no running game.

    ~/.local/opt/re-venv/bin/python -m unittest tools.difftest.test_av
"""

import unittest

from . import av, memread, trace


def rec(s, q, f, a, e=None, l=None):
    return {"s": s, "q": q, "f": f, "a": a, "e": e, "l": l, "t": 0, "ra": 0}


class Decode(unittest.TestCase):
    def test_places_invert_memreads(self):
        self.assertEqual({v: k for k, v in memread.WIDE_PLACES.items()}, av.PLACES)

    def test_battle_entries_as_seen_on_rk1(self):
        # РК1 step 21 (a shot at enemy front column 4) and step 22 (an enemy's blow on own
        # front column 4), as the trace logged them.
        recs = [
            rec(21, 1, "QueuePush", {"q": 0, "fn": "0x4afbd8"}, l={"entry": [0x4AFBD8, 0, 33751305, 0]}),
            rec(21, 2, "QueuePush", {"q": 0, "fn": "0x4afe7c"}, l={"entry": [0x4AFE7C, 0, 515, 0]}),
            rec(21, 3, "SoundPlay", {"slot": 28, "restart": 1, "loop": 0}, e={"group": 2, "name": "Battle-Shoot"}),
            rec(22, 4, "QueuePush", {"q": 0, "fn": "0x4afe7c"}, l={"entry": [0x4AFE7C, 0, 65795, 1]}),
            rec(22, 5, "SoundPlay", {"slot": 10, "restart": 1, "loop": 1}, e={"group": 1, "name": "BkgBattle1"}),
            rec(22, 6, "MusicPlay", {"track": 10}, e={"name": "BkgBattle1"}),
            rec(22, 7, "QueuePush", {"q": 0, "fn": "0x4af658"}, l={"entry": [0x4AF658, 0, 250, 0]}),
        ]
        ev = av.original_events(recs)
        self.assertEqual([(e["k"], e["n"], e.get("t")) for e in ev[21]],
                         [("anim", "battle_slide", "1:2:4"), ("anim", "battle_effect:shot", "2:1:4"),
                          ("sfx", "Battle-Shoot", None)])
        # The music's own Sound_Play (group 1) is not an effect; Music_Play gives the track.
        self.assertEqual([(e["k"], e["n"], e.get("t")) for e in ev[22]],
                         [("anim", "battle_effect:melee", "1:1:4"), ("music", "BkgBattle1", None),
                          ("anim", "chain", None)])


class Diff(unittest.TestCase):
    def test_missing_extra_order_and_internal(self):
        o = [{"k": "sfx", "n": "A"}, {"k": "sfx", "n": "B"}, {"k": "sfx", "n": "A"},
             {"k": "anim", "n": "chain"}]
        r = [{"k": "sfx", "n": "B"}, {"k": "sfx", "n": "A"}, {"k": "sfx", "n": "C"}]
        d = av.diff_step(o, r)
        self.assertEqual(set(d), {"sfx"})   # chain is internal
        self.assertEqual(d["sfx"]["missing"], ["A"])
        self.assertEqual(d["sfx"]["extra"], ["C"])
        self.assertTrue(d["sfx"]["order"])  # A B vs B A
        self.assertEqual(av.diff_step(r[:2], r[:2]), {})

    def test_battle_targets_compared_when_the_counts_agree(self):
        o = [{"k": "anim", "n": "battle_effect:melee", "t": "2:1:3"}]
        r = [{"k": "anim", "n": "battle_effect:melee", "t": "2:1:4"}]
        d = av.diff_step(o, r)
        self.assertEqual(d["anim"]["targets"], [("battle_effect:melee", ["2:1:3"], ["2:1:4"])])


class Preset(unittest.TestCase):
    def test_av_preset_hooks_and_names(self):
        specs = trace.specs_for("av")
        self.assertEqual(sorted(s["addr"] for s in specs),
                         [0x481420, 0x48C2E4, 0x48C348, 0x49D774, 0x49D7F8])
        names = [n for _, n in trace.SOUND_GLOBALS]
        self.assertEqual(len(names), len(set(names)))
        self.assertIn("Battle-Parry", av.SFX)
        self.assertEqual(len(av.MUSIC), 13)

    def test_coverage_rows_cover_every_name(self):
        rows = av.coverage_rows({("sfx", "Battle-Fight"): 2}, {("sfx", "Battle-Fight"): 1,
                                                               ("sfx", "Item-Gold"): 1})
        st = {(k, n): s for k, n, _, _, s in rows}
        self.assertEqual(st[("sfx", "Battle-Fight")], "both")
        self.assertEqual(st[("sfx", "Item-Gold")], "only Razdor")
        self.assertEqual(st[("sfx", "Battle-Parry")], "never triggered yet")
        self.assertEqual(st[("anim", "battle_effect:cure")], "never triggered yet")


if __name__ == "__main__":
    unittest.main()
