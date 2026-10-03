"""Minimal reader of Discord Times `.DTm` maps, enough to check the memory reader.

Layout: docs/reference/dtm-format.md. Only the header, the buildings and the armies are
decoded; terrain, objects, points, events and strings are skipped (their sizes are known).
"""

import bz2
import struct
from dataclasses import dataclass, field

HEADER = 0x12F
BUILDING = 358
ARMY = 89
EVENT = 171


@dataclass
class Building:
    id: int
    x: int  # bottom-right cell of the footprint, as in the file
    y: int
    type: int
    goods: list
    income: int
    max_gold: int
    size_x: int
    size_y: int
    owner: int
    garrison: list  # (unit, level, count), unit 1-based
    faction: int
    mana_income: int
    mana_max: int
    start_for_class: list


@dataclass
class Army:
    id: int
    x: int
    y: int
    gold: int
    home: int
    leader: int  # 1-based unit type, 0 none
    leader_level: int
    troops: list  # (unit, level, count)
    inactive: int


@dataclass
class HeroPreset:
    gold: int
    mana: int
    building: int
    troops: list
    x: int
    y: int


@dataclass
class Map:
    width: int
    height: int
    start_minutes: int
    kind: int
    presets: list
    buildings: list = field(default_factory=list)
    armies: list = field(default_factory=list)
    event_count: int = 0


def _payload(path):
    raw = open(path, "rb").read()
    if raw[0:1] != b"A" or raw[2:4] != b"pf":
        raise ValueError(f"{path}: not a DTm container")
    size = struct.unpack_from("<I", raw, 8)[0]
    data = bz2.decompress(raw[12:])
    if len(data) != size:
        raise ValueError(f"{path}: payload is {len(data)} bytes, header says {size}")
    return data


def _triples(b, off, n=6):
    out = []
    for i in range(n):
        u, a, c = b[off + 3 * i : off + 3 * i + 3]
        if u:
            out.append((u, a, c))
    return out


def read(path):
    d = _payload(path)
    u32 = lambda o: struct.unpack_from("<I", d, o)[0]
    w, h = u32(0x0C), u32(0x10)
    sizes = [u32(o) for o in (0x1C, 0x20, 0x24, 0x28, 0x2C, 0x30)]
    presets = []
    for k in range(3):
        p = 0x3C + 50 * k
        presets.append(HeroPreset(
            gold=struct.unpack_from("<h", d, p + 8)[0],
            mana=struct.unpack_from("<h", d, p + 12)[0],
            building=d[p + 16],
            troops=_triples(d, p + 19),
            x=struct.unpack_from("<H", d, p + 37)[0],
            y=struct.unpack_from("<H", d, p + 39)[0],
        ))
    m = Map(width=w, height=h, start_minutes=u32(0x38), kind=d[0x10F], presets=presets)
    off = HEADER + sizes[0] + sizes[1]
    for i in range(sizes[2] // BUILDING):
        b = d[off + i * BUILDING : off + (i + 1) * BUILDING]
        m.buildings.append(Building(
            id=i + 1,
            x=struct.unpack_from("<H", b, 0)[0],
            y=struct.unpack_from("<H", b, 2)[0],
            type=b[6],
            goods=list(struct.unpack_from("<12h", b, 136)),
            income=struct.unpack_from("<H", b, 282)[0],
            max_gold=struct.unpack_from("<H", b, 284)[0],
            size_x=b[289],
            size_y=b[290],
            owner=b[292],
            garrison=_triples(b, 314),
            faction=b[337],
            mana_income=b[350],
            mana_max=b[351],
            start_for_class=list(b[353:356]),
        ))
    off += sizes[2]
    for i in range(sizes[3] // ARMY):
        r = d[off + i * ARMY : off + (i + 1) * ARMY]
        m.armies.append(Army(
            id=i + 1,
            x=struct.unpack_from("<H", r, 0)[0],
            y=struct.unpack_from("<H", r, 2)[0],
            gold=struct.unpack_from("<h", r, 17)[0],
            home=r[25],
            leader=r[26],
            leader_level=r[27],
            troops=_triples(r, 28),
            inactive=r[63],
        ))
    m.event_count = sizes[5] // EVENT
    return m
