"""Checks of the differ and the battle layout that need no running game.

    ~/.local/opt/re-venv/bin/python -m unittest tools.difftest.test_run
"""

import struct
import unittest

from . import memread, run


class Generator(unittest.TestCase):
    def test_offset_both_ways(self):
        s = 1
        steps = [s]
        for _ in range(10):
            steps.append(run.lcg(steps[-1]))
        self.assertEqual(run.lcg_offset(steps[2], steps[7]), 5)
        self.assertEqual(run.lcg_offset(steps[7], steps[2]), -5)
        self.assertEqual(run.lcg_offset(steps[3], steps[3]), 0)
        self.assertIsNone(run.lcg_offset(1, 12345, limit=50))

    def test_engine_vectors(self):
        # engine.md §3.1: from S = 1 the 15-bit outputs are 41, 18467, 6334.
        s, out = 1, []
        for _ in range(3):
            s = run.lcg(s)
            out.append((s >> 16) & 0x7FFF)
        self.assertEqual(out, [41, 18467, 6334])


class Compare(unittest.TestCase):
    def test_fields_both_have(self):
        a = {"rng": 1, "hero": {"x": 1, "units": [{"type": 1, "hp": 5}]},
             "armies": [{"id": 3, "alive": False}]}
        b = {"rng": 2, "hero": {"x": 1, "units": [{"type": 1, "hp": 6}]},
             "armies": [{"id": 3, "alive": False, "x": 4}], "battle": {"turn": 1}}
        diffs, missing = run.compare(a, b)
        self.assertEqual(sorted(d[0] for d in diffs), ["hero.units[0].hp", "rng"])
        self.assertIn(("armies[id 3].x", "original"), missing)
        self.assertIn(("battle", "original"), missing)

    def test_list_length(self):
        diffs, _ = run.compare({"u": [{"a": 1}]}, {"u": [{"a": 1}, {"a": 2}]})
        self.assertEqual(diffs, [("u.len", 1, 2)])

    def test_shown_event_counts_as_done(self):
        st = {"events_done": [1]}
        meta = {"screen": "event", "dialog_event": 16, "event_count": 54}
        self.assertEqual(run.normalise_original(st, meta)["events_done"], [1, 17])
        # The engine's own report uses the slot past the last event: not an event.
        meta = {"screen": "event", "dialog_event": 54, "event_count": 54}
        self.assertEqual(run.normalise_original(st, meta)["events_done"], [1])

    def test_window_timing(self):
        rows = [
            {"step": 0, "action": {"op": "click_map"}, "diffs": [("hero.gold", 140, 100), ("rng", 1, 2)]},
            {"step": 1, "action": {"op": "ok"}, "diffs": [("hero.gold", 140, 100)]},
            {"step": 2, "action": {"op": "ok"}, "diffs": []},
        ]
        log = {0: {"meta": {"screen": "event"}}, 1: {"meta": {"screen": "village"}},
               2: {"meta": {"screen": "world"}}}
        run.mark_window_timing(rows, log)
        self.assertEqual([d[0] for d in rows[0]["diffs"]], ["rng"])
        self.assertEqual([d[0] for d in rows[0]["timing"]], ["hero.gold"])
        self.assertEqual(rows[1]["diffs"], [])


class BattleLayout(unittest.TestCase):
    def test_wide_places(self):
        # Formation_CellToSlot (0x492940) with 6 columns: key col*16+row.
        p = memread.WIDE_PLACES
        self.assertEqual([p[(1, c)] for c in range(1, 7)], [0, 1, 2, 3, 4, 5])
        self.assertEqual([p[(2, c)] for c in range(2, 6)], [7, 8, 9, 10])
        self.assertEqual((p[(3, 3)], p[(3, 4)]), (6, 11))
        self.assertEqual(sorted(p.values()), list(range(12)))

    def test_trace_stub_returns_to_random(self):
        stub = memread.DrawTrace.STUB
        # The displaced prologue, then a jump back past the patched bytes.
        self.assertEqual(stub[-11:-5], memread.DrawTrace.PROLOGUE)
        rel = struct.unpack("<i", stub[-4:])[0]
        self.assertEqual(memread.TRACE_CAVE + len(stub) + rel, memread.RANDOM + 6)
        patch = memread.DrawTrace.PATCH
        self.assertEqual(memread.RANDOM + 5 + struct.unpack("<i", patch[1:5])[0], memread.TRACE_CAVE)

    def test_draw_sites(self):
        self.assertEqual(memread.draw_site(0x4AD933), "patroller idle offset")
        self.assertEqual(memread.draw_site(0x4D1663), "window chord")
        self.assertEqual(memread.draw_site(0x401000), "?")


if __name__ == "__main__":
    unittest.main()
