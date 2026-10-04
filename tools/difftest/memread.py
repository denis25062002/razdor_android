"""Read the game state of a running DiscordTimes.exe (under Wine) from /proc/<pid>/mem.

The reader only looks at the globals of the original (image base 0x400000, BSS
0x4ee000-0xc0a891). Two writes exist, both off unless asked for: the draw trace's hook
(`DrawTrace`) and the music timer (`Game.hold_music`). Addresses and record layouts come
from the reverse-engineering notes (docs/reference/original-mechanics/*.md give the public subset);
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
ENTERED = 0x68DC74       # i32 the building the hero is in (entered), 0 none (world.md §7.2)
SCREEN = 0x4ECDC4         # ptr current screen (window) object
MINIMAP_SHOWN = 0x68DC64  # u8
NEXT_MUSIC = 0xAE123C     # i32 time (ms) of the next music change, which draws the RNG
NOW_MS = 0x4F1C34         # u32 the frame clock (ms) the music timer is compared with
INPUT_ON = 0x68DC63       # u8 1 while the battle window takes the player's input
FORMATION_COLS = 0x4ED044 # i32 6 (Community wide row) or 4 (vanilla)
PACK = 0x68DCE0           # i32[256] pack: item GlobalIndex, 0 empty (holes stay)
PACK_SCROLL = 0x68E0E4    # i32 first shown pack slot of the army / hero window
BOOK, BOOK_COUNT = 0x68E0E8, 0x68E4E8   # i32[] spell book (1-based spell numbers), count
HELD = 0x6664A4           # i32 item held on the pointer (army / hero window)
SPELL_PENDING = 0x66C0B9  # u8 a world spell is being targeted or cast
TAB = 0x68DC88            # i32 building window tab: 0 hall, 1 hire, 2 garrison, 3 market, 4 sanctuary
GRID = 0x1630             # army formation: cell (row r, col c) at +0x1630 + r*0x18 + c*4 = unit number

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

# Battle (battle.md): the battle object, its two sides and their unit records.
BATTLE = 0x668CF8         # +0xd turn, +0x12 cursor side, +0x16 cursor index, +0x22 over
SIDE_STRIDE = 0x851       # side s header at BATTLE + s*0x851 - 0x82e; +0 unit count
BUNIT = 0xA5              # unit u (1-based) at header + 0x28 + (u-1)*0xa5
# Battle cards (widgets, as the event buttons): 12 enemy cards, 12 own cards, by screen place.
ENEMY_CARDS, OWN_CARDS, CARD_STRIDE = 0x66AEA0, 0x66B224, 0x4B
# Screen place of a grid cell (row, col) in the 6-column formation (Formation_CellToSlot
# 0x492940): the front row is places 0-5, the back row 7-10, the reserve the ends 6 and 11.
WIDE_PLACES = {(1, c): c - 1 for c in range(1, 7)}
WIDE_PLACES.update({(2, c): c + 5 for c in range(2, 6)})
WIDE_PLACES.update({(3, 3): 6, (3, 4): 11})

# A trace of the generator's draws (optional, see DrawTrace): a hook on Random (0x4832fc)
# that logs each call into a ring in unused space at the end of the Community's .mod section.
RANDOM = 0x4832FC
TRACE_CAVE, TRACE_COUNT, TRACE_RING, TRACE_SLOTS = 0xC2B000, 0xC2B100, 0xC2B200, 512

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

    def write(self, addr, data):
        """Writes into the process (only the draw trace and the music timer do this)."""
        if not hasattr(self, "w"):
            self.w = open(f"/proc/{self.pid}/mem", "r+b", buffering=0)
        self.w.seek(addr)
        self.w.write(data)

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
                rec["items"] = list(struct.unpack("<4i", self.m.read(u + 0xCD, 16)))
            out.append(rec)
        return out

    def hero(self):
        a = self.army(0)
        return {"x": self.m.i32(a + 0x1724), "y": self.m.i32(a + 0x1728),
                "gold": self.m.i32(GOLD), "mana": self.m.i32(MANA),
                "units": self.units(0, xp=True), "pack": [i for i in self.pack() if i],
                "book": self.book()}

    def pack(self):
        """The 256 pack slots (0 = empty; a sale leaves a hole that the next item fills)."""
        return list(struct.unpack("<256i", self.m.read(PACK, 1024)))

    def book(self):
        n = max(0, min(self.m.i32(BOOK_COUNT), 256))
        return list(struct.unpack(f"<{n}i", self.m.read(BOOK, 4 * n)))

    def unit_cell(self, k, i):
        """(row, col) of unit i (0-based, record order) in army k's formation, or None."""
        a = self.army(k)
        for r in (1, 2, 3):
            for c in range(1, 7):
                if self.m.i32(a + GRID + r * 0x18 + c * 4) == i + 1:
                    return r, c
        return None

    def enabled(self, addr):
        """A widget's enabled flag (+0xd)."""
        return self.m.u8(addr + 0xD) != 0

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

    def battle(self):
        """The battle on screen (schema v1 `battle`): turn, the acting unit while the player
        has the input, and both sides' unit records in list order."""
        b = BATTLE
        sides = []
        for s in (1, 2):
            h = b + s * SIDE_STRIDE - 0x82E
            units = []
            for u in range(max(0, min(self.m.i32(h), 12))):
                r = h + 0x28 + u * BUNIT
                units.append({"type": self.m.i32(r + 0x23), "row": self.m.i32(r + 0x75),
                              "col": self.m.i32(r + 0x79), "hp": self.m.i32(r + 0x7D),
                              "actions": self.m.i32(r + 0x91)})
            sides.append(units)
        out = {"turn": self.m.i32(b + 0xD), "sides": sides}
        cs, ci = self.m.i32(b + 0x12), self.m.i32(b + 0x16)
        if self.m.u8(INPUT_ON) and not self.m.u8(b + 0x22) and cs in (1, 2) and \
                1 <= ci <= len(sides[cs - 1]):
            u = sides[cs - 1][ci - 1]
            out["actor"] = [cs, u["row"], u["col"]]
        return out

    def battle_raw(self):
        """Facts of the battle units outside the schema, for the run log: per side, per unit
        (record order) its attack/defence/initiative fields and this turn's modifiers."""
        names = {"ab": 0x2C, "as": 0x30, "mp": 0x34, "db": 0x38, "ds": 0x3C, "maxhp": 0x4C,
                 "manevres": 0x50, "init": 0x54, "atk_mod": 0x81, "def_mod": 0x85,
                 "init_mod": 0x89, "bld_def": 0x8D, "cur_init": 0x95}
        out = []
        for s in (1, 2):
            h = BATTLE + s * SIDE_STRIDE - 0x82E
            side = []
            for u in range(max(0, min(self.m.i32(h), 12))):
                r = h + 0x28 + u * BUNIT
                side.append({k: self.m.i32(r + o) for k, o in names.items()} |
                            {"bonus": self.m.u8(r + 0x62), "role": self.m.u8(r + 0x68)})
            out.append(side)
        return {"sides": out, "threshold": self.m.i32(BATTLE + 0x1A)}

    def card(self, side, row, col):
        """(x, y, w, h) of the battle card of grid cell (row, col) of side 1 (the player's) or
        2, or None when the cell has no place on screen."""
        if self.m.i32(FORMATION_COLS) != 6:
            return None  # the vanilla 3x4 place table (jump table 0x492ab1) is not mapped here
        place = WIDE_PLACES.get((row, col))
        if place is None:
            return None
        hidden, x, y, w, h = self.widget((OWN_CARDS if side == 1 else ENEMY_CARDS) + place * CARD_STRIDE)
        return None if w <= 0 or h <= 0 else (x, y, w, h)

    def state(self, step, map_name):
        st = {"step": step, "map": map_name, "clock": self.clock(), "rng": self.rng(),
              "hero": self.hero(), "armies": self.armies(), "buildings": self.buildings(),
              "events_done": self.events_done()}
        if self.screen() == "battle":
            st["battle"] = self.battle()
        return st

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
                "hero_class": self.m.i32(HERO_CLASS), "next_music_ms": self.m.i32(NEXT_MUSIC),
                "now_ms": self.m.u32(NOW_MS), "event_count": self.m.i32(EVENT_COUNT),
                "entered": self.m.i32(ENTERED)} | \
            ({"battle_raw": self.battle_raw()} if self.screen() == "battle" else {})

    def hold_music(self, ahead_ms=3_600_000):
        """Puts the next timed music change an hour ahead: the rotation draws the generator
        in real time (engine.md §3.4), which a replay cannot place."""
        self.m.write(NEXT_MUSIC, struct.pack("<i", (self.m.u32(NOW_MS) + ahead_ms) & 0x7FFFFFFF))


# Sites of the generator's callers (engine.md §3.4), for naming a traced draw by its
# return address: the nearest site at or below it.
DRAW_SITES = [
    (0x483344, "plant offset"), (0x4CFB24, "plant sway"), (0x4B4B43, "army idle offset"),
    (0x4B8691, "army idle offset (save load)"), (0x4AD8A0, "patroller idle offset"),
    (0x4BE178, "market restock"), (0x4A1998, "barracks"), (0x4BBA40, "village offer roll"),
    (0x4ACA80, "village offer build"), (0x4A2550, "AI wander points"),
    (0x4A4A7C, "AI promotion"), (0x4A548C, "AI hire XP"), (0x4AB150, "anti-cheat"),
    # Deep inside the arrival rules 0x4a548c (far past its first 0x400 bytes): the XP of a
    # unit an AI army hires (return 0x4a6b74) or buys for its garrison (return 0x4a7017).
    (0x4A6B74, "AI hire XP"), (0x4A7017, "AI hire XP (garrison)"),
    (0x4D1282, "window chord"), (0x4D155F, "window chord"), (0x4D165E, "window chord"),
    (0x49D774, "music"), (0x49D7F8, "music"), (0x486237, "battle AI noise"),
]


def draw_site(ret):
    best = None
    for addr, name in DRAW_SITES:
        if addr <= ret < addr + 0x400 and (best is None or addr > best[0]):
            best = (addr, name)
    return best[1] if best else "?"


class DrawTrace:
    """Logs every Random(n) of the original: a jump at the top of Random (0x4832fc) into a
    stub in unused, zero space at the end of the Community's .mod section (RWX), which stores
    (return address, n, state before, game time) into a 512-slot ring and counts the calls.
    The game's behaviour is unchanged (the displaced prologue runs in the stub)."""

    PATCH = b"\xe9" + struct.pack("<i", TRACE_CAVE - (RANDOM + 5)) + b"\x90"  # jmp stub; nop
    STUB = bytes.fromhex(
        "53518b1d00b1c20081e3ff010000c1e3048b4c2408898b00b2c200898304b2c2008b0d5491650089"
        "8b08b2c2008b0db8dc6800898b0cb2c200ff0500b1c200595b558bec83c4f8e9b68285ff")
    PROLOGUE = bytes.fromhex("558bec83c4f8")  # push ebp; mov ebp, esp; add esp, -8

    def __init__(self, mem):
        self.m = mem
        self.seen = 0

    def install(self):
        head = self.m.read(RANDOM, 6)
        if head == self.PATCH:
            self.seen = self.m.u32(TRACE_COUNT)
            return
        if head != self.PROLOGUE:
            raise RuntimeError(f"Random does not start as expected: {head.hex()}")
        if any(self.m.read(TRACE_CAVE, TRACE_RING + TRACE_SLOTS * 16 - TRACE_CAVE)):
            raise RuntimeError("the trace area is not free")
        self.m.write(TRACE_CAVE, self.STUB)
        self.m.write(RANDOM, self.PATCH)
        self.seen = 0

    def poll(self):
        """The draws since the last poll: [(n, state before, return address, time_cs)], and
        how many were lost to the ring wrapping."""
        count = self.m.u32(TRACE_COUNT)
        new = count - self.seen
        lost = max(0, new - TRACE_SLOTS)
        out = []
        for k in range(self.seen + lost, count):
            ret, n, before, t = struct.unpack("<IiIi", self.m.read(TRACE_RING + (k % TRACE_SLOTS) * 16, 16))
            out.append((n, before, ret, t))
        self.seen = count
        return out, lost


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
    # Events that fired during the load (the shown one counts its firing only when closed)
    # have already applied their results: units given to the hero, armies switched on/off.
    base = m.u32(EVENTS)
    fired = set(game.events_done())
    if game.dialog_event() >= 0:
        fired.add(game.dialog_event() + 1)
    given, switched = [], {}
    for ev in sorted(fired):
        e = base + (ev - 1) * EVENT_SIZE
        given += [u for u in m.read(e + 0x61, 4) if u]
        for k in m.read(e + 0x79, 2):
            if k:
                switched[k] = True
        if m.u8(e + 0x7B):
            switched[m.u8(e + 0x7B)] = False
    if fired:
        chk("events fired at load", True, f"{sorted(fired)}: units {given}, armies {switched}")
    h = game.hero()
    chk("hero x, y", (h["x"], h["y"]) == (p.x, p.y), f"{(h['x'], h['y'])} vs {(p.x, p.y)}")
    chk("hero cell table", m.i32(0x75A544) == p.y * m.i32(STRIDE_X) + p.x)
    chk("hero gold", h["gold"] == p.gold, f"{h['gold']} vs {p.gold}")
    chk("hero mana", h["mana"] == p.mana, f"{h['mana']} vs {p.mana}")
    want = [(u, lv) for (u, lv, c) in p.troops if u > 3 for _ in range(c)] + [(u, 0) for u in given]
    got = [(u["type"], u["level"]) for u in h["units"][1:]]
    chk("hero troops (type, level)", got == want, f"{got} vs {want}")
    chk("hero unit 1 is the class", h["units"][0]["type"] == cls + 1)

    armies = game.armies()
    chk("army count", len(armies) == len(mp.armies), f"{len(armies)} vs {len(mp.armies)}")
    for a, f in zip(armies, mp.armies):
        chk(f"army {f.id} x, y", (a["x"], a["y"]) == (f.x, f.y), f"{(a['x'], a['y'])} vs {(f.x, f.y)}")
        chk(f"army {f.id} gold", a["gold"] == f.gold, f"{a['gold']} vs {f.gold}")
        chk(f"army {f.id} active", a["active"] == switched.get(f.id, f.inactive == 0))
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
