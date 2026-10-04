"""Checks of the original side that need no running game.

    ~/.local/opt/re-venv/bin/python -m unittest tools.difftest.test_original
"""

import os
import unittest

from . import dtm, original

INSTALL = os.environ.get("RAZDOR_DT_DIR", original.DEFAULT_INSTALL)
RK1 = os.path.join(INSTALL, "Maps_Rus", "РК1-Начало пути.DTm")


class FakeGame:
    def __init__(self, camera, minimap=None):
        self._camera, self._minimap = camera, minimap

    def camera(self):
        return self._camera

    def minimap(self):
        return self._minimap


class PixelLayout(unittest.TestCase):
    def harness(self, camera, minimap=None):
        o = original.Original.__new__(original.Original)
        o.game = FakeGame(camera, minimap)
        return o

    def test_cell_to_pixel_inverts_the_cursor_formula(self):
        # Cursor cell = floor((camera + mouse) / size) - 1 (interface.md §7.1).
        o = self.harness((608, 440))
        for x, y in [(39, 43), (37, 39), (12, 8), (0, 0)]:
            px, py = o.cell_pixel(x, y)
            self.assertEqual(((608 + px) // 32 - 1, (440 + py) // 22 - 1), (x, y))
        self.assertEqual(o.cell_pixel(37, 39), (624, 451))

    def test_safe_area_avoids_the_minimap_and_the_edges(self):
        o = self.harness((0, 0), minimap=(810, 14, 200))
        self.assertTrue(o._in_safe(500, 300))
        self.assertFalse(o._in_safe(900, 100))   # under the minimap
        self.assertFalse(o._in_safe(10, 300))    # edge scrolling zone
        self.assertFalse(o._in_safe(500, 700))   # bottom panel


class MapNames(unittest.TestCase):
    entries = [(0, "Обучающий1.DTm", 1), (1, "РК1-Начало пути.DTm", 1), (9, "РК2-Дикие пустоши.DTm", 2)]

    def test_exact_suffix_and_prefix(self):
        m = lambda n: [e[0] for e in original.match_map(self.entries, n)]
        self.assertEqual(m("РК1-Начало пути.DTm"), [1])
        self.assertEqual(m("РК1-Начало пути"), [1])
        self.assertEqual(m("РК1"), [1])
        self.assertEqual(m("РК"), [])  # ambiguous


@unittest.skipUnless(os.path.exists(RK1), "needs the Discord Times install")
class MapReader(unittest.TestCase):
    def test_rk1(self):
        m = dtm.read(RK1)
        self.assertEqual((m.width, m.height, m.kind), (50, 50, 1))
        self.assertEqual(m.start_minutes, 624296700)  # 1204-04, day 9, 09:00
        self.assertEqual((m.presets[0].x, m.presets[0].y, m.presets[0].gold), (39, 43, 100))
        self.assertEqual((len(m.buildings), len(m.armies), m.event_count), (12, 15, 54))
        self.assertEqual((m.armies[0].x, m.armies[0].y, m.armies[0].gold), (10, 5, 200))


if __name__ == "__main__":
    unittest.main()
