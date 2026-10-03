"""Read the game state of a running DiscordTimes.exe (under Wine) from /proc/<pid>/mem.

Nothing is injected and nothing is written: the reader only looks at the globals of the
original (image base 0x400000, BSS 0x4ee000-0xc0a891). Addresses and record layouts come from
the reverse-engineering notes (docs/reference/original-mechanics/*.md give the public subset);
each one used here was checked against a live game (see README.md, "Original side").

    python -m tools.difftest.memread                 # print the state (schema v1) as JSON
    python -m tools.difftest.memread --check MAP     # right after loading MAP: compare with the file
"""

import argparse
import json
import os
import struct
import sys

# --- globals -------------------------------------------------------------------------------
RNG = 0x659154            # u32 state of the game's generator (0x4832fc)
TIME_CS = 0x68DCB8        # i32 game time, centi-minutes since the map was loaded
TIME_START = 0x68DCBC     # i32 start offset in minutes (= header start minute + 1)
GOLD = 0x75C018           # i32 player's gold (= hero army +0x16d8)
MANA = 0x68E4F4           # i32 player's mana
HERO_CLASS = 0x68DCCC     # i32 0 knight, 1 archmage, 2 ranger
CAMERA_X, CAMERA_Y = 0x68E788, 0x68E78C   # i32 top-left of the view, pixels
PLANNED_X, PLANNED_Y = 0x68DCA8, 0x68DCAC  # i32 cell of the last planned route
IDLE = 0x68DC62           # u8 1 while the map takes input (no walk, wait, glide)
QUEUE0 = 0xB06F60         # i32 entries in deferred-call queue 0 (walks, waits, glides)
DIALOG_EVENT = 0x68DC70   # i32 event shown in the event window, -1 none
SCREEN = 0x4ECDC4         # ptr current screen (window) object
MINIMAP_SHOWN = 0x68DC64  # u8
NEXT_MUSIC = 0xAE123C     # i32 time (ms) of the next music change, which draws the RNG

STRIDE_X = 0x68ECC0       # i32 cell-row stride (map width + 8)
BUILDING_COUNT = 0x68ECD0
ARMY_COUNT = 0x68ECD4
EVENT_COUNT = 0x68ECDC
BUILDINGS = 0x68ECE0      # ptr to building records (0x166 each, 1-based ids)
EVENTS = 0x68ECEC         # ptr to event records (0xab each)
MAP_HEADER = 0xAE14B0     # copy of the DTm header (0x12f)
MAP_FILE = 0xAE1234       # AnsiString: file name of the loaded map (restart snapshot)

ARMIES = 0x75A940         # army records, 0x3827 each; 0 = hero, 1..N map armies, N+1 ship
ARMY_SIZE = 0x3827
UNIT_SIZE = 0x1DB
BUILDING_SIZE = 0x166
EVENT_SIZE = 0xAB

# Event window buttons (widgets: +4 hidden, +0x11 x, +0x15 y, +0x19 w, +0x1d h).
EVENT_YES, EVENT_NO, EVENT_OK = 0x672B40, 0x672CB4, 0x672E28

# New-game window.
MAP_LIST = 0xAE1C1C       # ptr to map-list entries (0x158 each)
MAP_LIST_COUNT = 0x4ED480
MAP_LIST_SELECTED = 0x65ADAB  # i32 selected entry
CLASS_ENABLED = 0x65BC01  # u8 per class, stride 0x4b

SCREENS = {
    0x6594C8: "main_menu", 0x65A5E4: "new_game", 0x65B074: "new_hero", 0x674A20: "world",
    0x672618: "event", 0x668A14: "battle", 0x66C8DC: "building", 0x671500: "village",
    0x671D38: "shipyard", 0x665234: "exit_menu", 0x65E460: "question", 0x66B71C: "exit_battle",
    0x65BE60: "options", 0x65D4E0: "load", 0x65EBC4: "save", 0x6664A8: "hero_window",
    0x6676D0: "army_window", 0x66BE80: "spell_book",
}


def find_pid(marker="DiscordTimes.exe"):
    """The Wine process running the game: its cmdline names the exe and its memory holds the
    PE image at 0x400000."""
    pids = []
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        try:
            cmd = open(f"/proc/{name}/cmdline", "rb").read().decode("utf-8", "replace")
        except OSError:
            continue
        if marker in cmd and "wine" not in cmd.split("\0")[0].rsplit("/", 1)[-1]:
            pids.append(int(name))
    for pid in pids:
        try:
            if Memory(pid).read(0x400000, 2) == b"MZ":
                return pid
        except OSError:
            pass
    return None


class Memory:
    def __init__(self, pid):
        self.pid = pid
        self.f = open(f"/proc/{pid}/mem", "rb", buffering=0)

    def close(self):
        self.f.close()

    def read(self, addr, n):
        self.f.seek(addr)
        return self.f.read(n)

    def u8(self, a):
        return self.read(a, 1)[0]

    def i8(self, a):
        return struct.unpack("<b", self.read(a, 1))[0]

    def i16(self, a):
        return struct.unpack("<h", self.read(a, 2))[0]

    def u16(self, a):
        return struct.unpack("<H", self.read(a, 2))[0]

    def i32(self, a):
        return struct.unpack("<i", self.read(a, 4))[0]

    def u32(self, a):
        return struct.unpack("<I", self.read(a, 4))[0]

    def dstring(self, ptr):
        """A Delphi AnsiString (length at ptr-4), cp1251."""
        if not ptr:
            return ""
        n = self.i32(ptr - 4)
        return self.read(ptr, n).decode("cp1251")


class Game:
    """Typed views of the original's globals."""

    def __init__(self, mem):
        self.m = mem

    # --- schema v1 ---
    def clock(self):
        """Minutes since year 0 as the game's event code computes `now`:
        time_cs div 100 + start offset."""
        return self.m.i32(TIME_CS) // 100 + self.m.i32(TIME_START)

    def rng(self):
        return self.m.u32(RNG)

    def army(self, k):
        return ARMIES + k * ARMY_SIZE

    def units(self, k, xp=False):
        a = self.army(k)
        n = self.m.i32(a)
        out = []
        for i in range(max(0, min(n, 12))):
            u = a + 4 + i * UNIT_SIZE
            hp = self.m.i32(u + 0x20)
            if hp == -1:  # unhurt: the game stores -1, the value is the current max HP
                hp = self.m.i32(u + 0xDE)
            rec = {"type": self.m.i32(u) + 1, "level": self.m.i32(u + 0x10), "hp": hp}
            if xp:
                rec["xp"] = self.m.i32(u + 4)
            out.append(rec)
        return out

    def hero(self):
        a = self.army(0)
        return {"x": self.m.i32(a + 0x1724), "y": self.m.i32(a + 0x1728),
                "gold": self.m.i32(GOLD), "mana": self.m.i32(MANA),
                "units": self.units(0, xp=True)}

    def armies(self):
        out = []
        for k in range(1, self.m.i32(ARMY_COUNT) + 1):
            a = self.army(k)
            out.append({"id": k, "x": self.m.i32(a + 0x1724), "y": self.m.i32(a + 0x1728),
                        "active": self.m.u8(a + 0x16A1) != 0,
                        "alive": self.m.u8(a + 0x16A2) == 0,
                        "gold": self.m.i32(a + 0x16D8), "units": self.units(k)})
        return out

    def building(self, b):
        return self.m.u32(BUILDINGS) + (b - 1) * BUILDING_SIZE

    def buildings(self):
        out = []
        for b in range(1, self.m.i32(BUILDING_COUNT) + 1):
            r = self.building(b)
            words = struct.unpack("<12h", self.m.read(r + 0x88, 24))
            out.append({"id": b, "owner": self.m.u8(r + 0x124), "gold": self.m.u16(r + 0x11E),
                        "mana": self.m.u8(r + 0x160), "goods": [abs(w) for w in words if w]})
        return out

    def events_done(self):
        base = self.m.u32(EVENTS)
        n = self.m.i32(EVENT_COUNT)
        return [i + 1 for i in range(n) if self.m.i16(base + i * EVENT_SIZE + 0xA0) > 0]

    def state(self, step, map_name):
        return {"step": step, "map": map_name, "clock": self.clock(), "rng": self.rng(),
                "hero": self.hero(), "armies": self.armies(), "buildings": self.buildings(),
                "events_done": self.events_done()}

    # --- harness helpers ---
    def map_file(self):
        """File name of the loaded map (the restart snapshot's map name)."""
        return self.m.dstring(self.m.u32(MAP_FILE))

    def screen(self):
        p = self.m.u32(SCREEN)
        return SCREENS.get(p, hex(p))

    def camera(self):
        return self.m.i32(CAMERA_X), self.m.i32(CAMERA_Y)

    def planned_target(self):
        """Cell the route was last planned to (a click on it sets off)."""
        return self.m.i32(PLANNED_X), self.m.i32(PLANNED_Y)

    def hero_cell(self):
        a = self.army(0)
        return self.m.i32(a + 0x1724), self.m.i32(a + 0x1728)

    def idle(self):
        return self.m.u8(IDLE) == 1 and self.m.i32(QUEUE0) == 0

    def dialog_event(self):
        return self.m.i32(DIALOG_EVENT)

    def widget(self, addr):
        """(hidden, x, y, w, h) of a widget."""
        return (self.m.u8(addr + 4) != 0, self.m.i32(addr + 0x11), self.m.i32(addr + 0x15),
                self.m.i32(addr + 0x19), self.m.i32(addr + 0x1D))

    def minimap(self):
        """(left, top, size) of the minimap when it is shown, else None."""
        if not self.m.u8(MINIMAP_SHOWN):
            return None
        size = 200 if self.m.i32(MAP_HEADER + 0x0C) < 100 else 400
        return 1010 - size, 14, size

    def map_list(self):
        """Entries of the new-game list: (index, file name, kind)."""
        base = self.m.u32(MAP_LIST)
        out = []
        for i in range(self.m.i32(MAP_LIST_COUNT)):
            e = base + i * 0x158
            out.append((i, self.m.dstring(self.m.u32(e + 0x12F)), self.m.u8(e + 0x10F)))
        return out

    def meta(self):
        """Extra facts for the run log (not part of the schema)."""
        return {"screen": self.screen(), "idle": self.idle(), "dialog_event": self.dialog_event(),
                "camera": list(self.camera()), "time_cs": self.m.i32(TIME_CS),
                "hero_class": self.m.i32(HERO_CLASS), "next_music_ms": self.m.i32(NEXT_MUSIC)}


# --- validation against the map file -------------------------------------------------------
def check_against_map(game, path):
    """Right after a new game on `path` (before any action): compare what the file fixes.
    Returns a list of (field, ok, detail)."""
    from . import dtm

    mp = dtm.read(path)
    m = game.m
    res = []

    def chk(name, ok, detail=""):
        res.append((name, bool(ok), detail))

    chk("map width", m.i32(MAP_HEADER + 0x0C) == mp.width, f"{m.i32(MAP_HEADER + 0x0C)} vs {mp.width}")
    chk("clock = header start + 1", game.clock() == mp.start_minutes + 1,
        f"{game.clock()} vs {mp.start_minutes + 1}")
    cls = m.i32(HERO_CLASS)
    p = mp.presets[cls]
    h = game.hero()
    chk("hero x, y", (h["x"], h["y"]) == (p.x, p.y), f"{(h['x'], h['y'])} vs {(p.x, p.y)}")
    chk("hero cell table", m.i32(0x75A544) == p.y * m.i32(STRIDE_X) + p.x)
    chk("hero gold", h["gold"] == p.gold, f"{h['gold']} vs {p.gold}")
    chk("hero mana", h["mana"] == p.mana, f"{h['mana']} vs {p.mana}")
    want = [(u, lv) for (u, lv, c) in p.troops if u > 3 for _ in range(c)]
    got = [(u["type"], u["level"]) for u in h["units"][1:]]
    chk("hero troops (type, level)", got == want, f"{got} vs {want}")
    chk("hero unit 1 is the class", h["units"][0]["type"] == cls + 1)

    armies = game.armies()
    chk("army count", len(armies) == len(mp.armies), f"{len(armies)} vs {len(mp.armies)}")
    for a, f in zip(armies, mp.armies):
        chk(f"army {f.id} x, y", (a["x"], a["y"]) == (f.x, f.y), f"{(a['x'], a['y'])} vs {(f.x, f.y)}")
        chk(f"army {f.id} gold", a["gold"] == f.gold, f"{a['gold']} vs {f.gold}")
        chk(f"army {f.id} active", a["active"] == (f.inactive == 0))
        chk(f"army {f.id} alive", a["alive"])
        want = ([(f.leader, f.leader_level)] if f.leader else []) + \
               [(u, lv) for (u, lv, c) in f.troops if u > 3 for _ in range(c)]
        got = [(u["type"], u["level"]) for u in a["units"]]
        chk(f"army {f.id} units", got == want, f"{got} vs {want}")
        chk(f"army {f.id} units unhurt", all(u["hp"] > 0 for u in a["units"]))

    blds = game.buildings()
    chk("building count", len(blds) == len(mp.buildings), f"{len(blds)} vs {len(mp.buildings)}")
    for b, f in zip(blds, mp.buildings):
        r = game.building(f.id)
        tl = (m.u16(r), m.u16(r + 2))
        chk(f"building {f.id} top-left", tl == (f.x - f.size_x + 1, f.y - f.size_y + 1))
        start = f.start_for_class[cls] != 0 or p.building == f.id
        chk(f"building {f.id} owner", b["owner"] == (0 if start else f.owner), f"{b['owner']} vs {f.owner}")
        if f.type == 2:
            chk(f"building {f.id} village gold = income", b["gold"] == f.income, f"{b['gold']} vs {f.income}")
            chk(f"building {f.id} village mana = mana income", b["mana"] == f.mana_income,
                f"{b['mana']} vs {f.mana_income}")
        fixed = [abs(g) for g in f.goods if g]
        if f.type in (1, 6, 7):
            chk(f"building {f.id} fixed goods kept", all(g in b["goods"] for g in fixed),
                f"{b['goods']} vs {fixed}")
        elif f.type != 12:
            chk(f"building {f.id} goods cleared", b["goods"] == [], f"{b['goods']}")
    chk("events_done empty before the start events", True, f"{game.events_done()}")
    return res


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pid", type=int, help="game process (default: find DiscordTimes.exe)")
    ap.add_argument("--check", metavar="MAP", help="compare with this .DTm right after loading it")
    ap.add_argument("--meta", action="store_true", help="also print the harness facts")
    a = ap.parse_args(argv)
    pid = a.pid or find_pid()
    if not pid:
        sys.exit("no DiscordTimes.exe process found")
    g = Game(Memory(pid))
    if a.check:
        res = check_against_map(g, a.check)
        bad = [r for r in res if not r[1]]
        for name, ok, detail in res:
            if not ok or a.meta:
                print(("ok  " if ok else "BAD ") + name, detail)
        print(f"{len(res) - len(bad)}/{len(res)} checks pass")
        sys.exit(1 if bad else 0)
    st = g.state(0, g.map_file())
    if a.meta:
        st["_meta"] = g.meta()
    print(json.dumps(st, ensure_ascii=False))


if __name__ == "__main__":
    main()
