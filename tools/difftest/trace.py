"""A runtime trace of the original Discord Times with Frida: any function, its arguments,
chosen memory before and after, and its return value, logged as JSON lines with a step tag.

How it gets in: the game runs under Wine's new-style WoW64 (32-bit code inside a 64-bit
Linux process), where a Linux Frida cannot attach (see README.md, "Runtime trace"). So the
Windows x86 frida-gadget DLL is copied into the harness's private copy of the install with a
config that makes it listen on 127.0.0.1, and the game is made to load it: the harness writes
a one-shot stub into unused space of the Community's `.mod` section (0xc2e000) and points the
import slot of timeGetTime (0xc0b834, called every frame) at it; the stub puts the slot back,
calls LoadLibraryA("frida-gadget.dll") and goes on into timeGetTime. Then the harness
connects to the gadget and loads `trace_agent.js`, which hooks the functions of the chosen
presets (or a JSON file of hook specs, see trace_agent.js).

    python -m tools.difftest.trace --list
    python -m tools.difftest.trace --read out/trace.jsonl --steps 8-10 --army 1
    python -m tools.difftest.original --actions a.jsonl --out out/ --trace random,ai:1
    python -m tools.difftest.run --actions a.jsonl --trace random,ai:1,events

The records go to `<out>/trace.jsonl`: {"s": step, "q": sequence, "f": hook name,
"t": game time (centi-minutes) at entry, "tl": at return, "ra": return address,
"a": arguments, "e": values read at entry, "l": values read at return, "r": return value}.
Step -1 is everything before the first action.

Setup: `pip install frida==<version>` in the venv that runs the harness, and the gadget of
the same version from https://github.com/frida/frida/releases
(frida-gadget-<version>-windows-x86.dll.xz, unpacked) at ~/.local/opt/frida-win/ or given
with `--gadget` / RAZDOR_FRIDA_GADGET.
"""

import argparse
import glob
import json
import os
import socket
import struct
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
AGENT = os.path.join(HERE, "trace_agent.js")

# --- loading the gadget -----------------------------------------------------------------------
IAT_TIMEGETTIME = 0xC0B834   # import slot of winmm!timeGetTime
IAT_LOADLIBRARYA = 0xC0B3A4  # import slot of kernel32!LoadLibraryA
LOADER = 0xC2E000            # stub; free, zero space in .mod (0xc2aa18-0xc35000 is unused)
LOADER_SAVED, LOADER_RESULT, LOADER_ENTERED, LOADER_NAME = 0xC2E080, 0xC2E084, 0xC2E088, 0xC2E090
GADGET_NAME = "frida-gadget.dll"


def loader_stub():
    p = lambda v: struct.pack("<I", v)
    return (b"\x60\x9c"                                        # pushad; pushfd
            + b"\xc7\x05" + p(LOADER_ENTERED) + p(1)          # mov [entered], 1
            + b"\xa1" + p(LOADER_SAVED)                         # mov eax, [saved]
            + b"\xa3" + p(IAT_TIMEGETTIME)                      # mov [iat], eax   (one shot)
            + b"\x68" + p(LOADER_NAME)                          # push name
            + b"\xff\x15" + p(IAT_LOADLIBRARYA)                 # call [LoadLibraryA]
            + b"\xa3" + p(LOADER_RESULT)                        # mov [result], eax
            + b"\x9d\x61"                                       # popfd; popad
            + b"\xff\x25" + p(IAT_TIMEGETTIME))                 # jmp [iat]


def find_gadget(path=None):
    import frida
    cands = [path, os.environ.get("RAZDOR_FRIDA_GADGET")]
    cands += sorted(glob.glob(os.path.expanduser(
        f"~/.local/opt/frida-win/frida-gadget-{frida.__version__}-windows-x86.dll")))
    for c in cands:
        if c and os.path.exists(c):
            return c
    raise RuntimeError(f"no Windows x86 frida-gadget {frida.__version__} found (see trace.py)")


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def put_gadget(install_dir, gadget, port):
    """Copy the gadget into the private install copy with a config that listens on `port`."""
    import shutil
    dst = os.path.join(install_dir, GADGET_NAME)
    if not os.path.exists(dst) or os.path.getsize(dst) != os.path.getsize(gadget):
        shutil.copy2(gadget, dst)
    cfg = {"interaction": {"type": "listen", "address": "127.0.0.1", "port": port,
                           "on_load": "resume"}}
    with open(os.path.join(install_dir, "frida-gadget.config"), "w") as f:
        json.dump(cfg, f)


def load_gadget(mem, timeout=30):
    """Make the running game load the gadget (see the module doc). `mem` is a
    memread.Memory of the game."""
    if mem.u32(LOADER_RESULT):
        return mem.u32(LOADER_RESULT)  # already loaded in this process
    if any(mem.read(LOADER, 0xA0)):
        raise RuntimeError("the loader area is not free")
    stub = loader_stub()
    mem.write(LOADER, stub)
    mem.write(LOADER_NAME, GADGET_NAME.encode() + b"\0")
    mem.write(LOADER_SAVED, struct.pack("<I", mem.u32(IAT_TIMEGETTIME)))
    mem.write(IAT_TIMEGETTIME, struct.pack("<I", LOADER))
    t0 = time.time()
    while time.time() - t0 < timeout:
        h = mem.u32(LOADER_RESULT)
        if h:
            return h
        if mem.u32(LOADER_ENTERED) and time.time() - t0 > 10:
            break
        time.sleep(0.05)
    raise RuntimeError("the game did not load the gadget (LoadLibraryA returned 0 or never ran)")


# --- presets ----------------------------------------------------------------------------------
A = "army(a.k)"
ARMY_FIELDS = {   # army record fields of the AI's movement (ai.md §2, world.md §5)
    "x": f"i32({A}+0x1724)", "y": f"i32({A}+0x1728)", "dir": f"i32({A}+0x1710)",
    "cost": f"i32({A}+0x1714)", "play": f"i32({A}+0x1718)", "total": f"i32({A}+0x1698)",
    "bank": f"i32({A}+0x37e4)", "window": f"i32({A}+0x37e8)", "ready": f"u8({A}+0x37ec)",
    "moving": f"u8({A}+0x37ed)", "adv": f"u8({A}+0x3801)", "idx": f"i32({A}+0x3782)",
    "len": f"i32({A}+0x3776)", "count": f"i32({A}+0x377e)", "idle": f"i32({A}+0x381e)",
}
WANDER = {"wp": f"[0,1,2,3].map(i => [i32({A}+0x3790+8*i), i32({A}+0x3794+8*i)])"}
PATH = {"path": f"(() => {{ const p = u32({A}+0x3772), n = Math.min(i32({A}+0x3776), 12), o = [];"
                f" for (let i = 0; i < n; i++) o.push([u16(p+8*i), u16(p+2+8*i)]); return o; }})()"}
TICK = {"tick": "i32(0xc09254)", "new_tick": "u8(0xc09251)", "hero": "[i32(0x68dcb0), i32(0x68dcb4)]"}
# A battle unit record as 485908 gets it (live: the pointers are memread's records): type
# +0x23, HP +0x7d.
UNIT_AT = lambda p: {"type": f"i32({p}+0x23)", "hp": f"i32({p}+0x7d)"}


def _army_when(arg):
    return f"a.k == {int(arg)}" if arg not in (None, "") else None


def preset_random(arg=None):
    return [{"name": "Random", "addr": 0x4832FC, "args": [["n", "eax", "i32"]],
             "enter": {"before": "u32(0x659154)"}, "ret": "i32"}]


def preset_ai(arg=None):
    """The AI armies' movement: the step clock's step starts and arrivals (all frames with
    `ai_frames`), goal choice, wander points, arrival rules, the snap at the hero's stop.
    `ai:K` keeps army K only."""
    when = _army_when(arg)
    k = [["k", "eax", "i32"]]
    changed = "r != 0 || " + " || ".join(f"e.{f} !== l.{f}" for f in ("x", "y", "dir", "moving", "idx"))
    return [
        {"name": "AiStepClock", "addr": 0x4A399C, "args": k + [["dt", "edx", "i32"]],
         "when": when, "enter": ARMY_FIELDS | TICK, "leave": ARMY_FIELDS, "ret": "u8", "log_if": changed},
        {"name": "AiPlan", "addr": 0x4A2D88, "args": k, "when": when,
         "enter": {"x": ARMY_FIELDS["x"], "y": ARMY_FIELDS["y"]}, "leave": PATH | {"count": ARMY_FIELDS["count"]}},
        {"name": "AiWanderPoints", "addr": 0x4A2550, "args": k, "when": when, "leave": WANDER},
        {"name": "AiArrival", "addr": 0x4A548C, "args": k + [["p2", "edx", "i32"]], "when": when,
         "enter": {"x": ARMY_FIELDS["x"], "y": ARMY_FIELDS["y"]}, "ret": "u8"},
        {"name": "ArmiesSnap", "addr": 0x4AD8A0, "args": [], "ret": None,
         "enter": {"armies": "(() => { const o = {}; for (let k = 1; k <= i32(0x68ecd4); k++)"
                             " if (i32(army(k)+0x1718) > 0) o[k] = [i32(army(k)+0x1724),"
                             " i32(army(k)+0x1728), i32(army(k)+0x1718), i32(army(k)+0x1710)];"
                             " return o; })()"} | TICK},
    ]


def preset_ai_frames(arg=None):
    """Every call of the step clock (each frame of game time, per army): `ai_frames:K`."""
    return [{"name": "AiStepClock", "addr": 0x4A399C,
             "args": [["k", "eax", "i32"], ["dt", "edx", "i32"]], "when": _army_when(arg),
             "enter": ARMY_FIELDS | TICK, "leave": ARMY_FIELDS, "ret": "u8"}]


def preset_advance(arg=None):
    """World_AdvanceAI(dt), the per-frame driver of the AI (0x4ade3c)."""
    return [{"name": "WorldAdvanceAI", "addr": 0x4ADE3C, "args": [["dt", "eax", "i32"]],
             "enter": TICK, "ret": "u8"}]


def preset_damage(arg=None):
    """Physical damage 0x485908(B, kind, attacker, defender) and ApplyDamage 0x48a354(B, side,
    index, dmg)."""
    hp = "i32(a.b + a.side*0x851 - 0x82e + a.index*0xa5)"
    typ = "i32(a.b + a.side*0x851 - 0x8ab + a.index*0xa5 + 0x23)"
    return [
        {"name": "PhysicalDamage", "addr": 0x485908,
         "args": [["b", "eax", "u32"], ["kind", "edx", "u8"], ["att", "ecx", "u32"], ["def", "stack:0", "u32"]],
         "enter": {"att_" + k: v for k, v in UNIT_AT("a.att").items()} |
                  {"def_" + k: v for k, v in UNIT_AT("a.def").items()}, "ret": "i32"},
        {"name": "ApplyDamage", "addr": 0x48A354,
         "args": [["b", "eax", "u32"], ["side", "edx", "i32"], ["index", "ecx", "i32"], ["dmg", "stack:0", "i32"]],
         "enter": {"type": typ, "hp": hp}, "leave": {"hp": hp}},
    ]


def preset_events(arg=None):
    """Event scan 0x4abfbc (returns whether one fired), opening an event 0x4a8ae8(index, 0-based),
    finishing the shown one 0x4ab1ec."""
    return [
        {"name": "EventsScan", "addr": 0x4ABFBC, "args": [], "enter": {"hero": TICK["hero"]},
         "leave": {"shown": "i32(0x68dc70)"}, "ret": "u8"},
        {"name": "EventOpen", "addr": 0x4A8AE8, "args": [["event", "eax", "i32"]], "ret": None,
         "enter": {"hero": TICK["hero"]}},
        {"name": "EventFinish", "addr": 0x4AB1EC, "args": [], "ret": None,
         "enter": {"event": "i32(0x68dc70)"}},
    ]


# The sound handles the loader fills (0x4e2e80, interface.md §14): the global that holds each
# `_Sounds.ini` key's slot. MainMenuSelect-2/-3 share -1's slot; Battle-Parry is the
# Community's (no ini entry in the shipped install).
SOUND_GLOBALS = [
    (0xAE1268, "BkgMenuMain"), (0xAE126C, "BkgAuthors"), (0xAE1278, "BkgMap1"), (0xAE127C, "BkgMap2"),
    (0xAE1280, "BkgMap3"), (0xAE1284, "BkgMap4"), (0xAE1288, "BkgMap5"), (0xAE128C, "BkgMap6"),
    (0xAE1290, "BkgMap7"), (0xAE1294, "BkgBattle1"), (0xAE1298, "BkgBattle2"), (0xAE1270, "BkgTriumph"),
    (0xAE1274, "BkgDefeat"),
    (0xAE12A0, "InterfaceButtonDown"), (0xAE1264, "InterfacePanelDown"), (0xAE1260, "InterfaceCastSpell"),
    (0xAE12A4, "InterfaceBarScroll"), (0xAE1244, "MainMenuSelect-1"), (0xAE125C, "MainMenuPress"),
    (0xAE1250, "Global-Event-1"), (0xAE1254, "Global-Event-2"), (0xAE1258, "Global-Event-3"),
    (0xAE12AC, "Global-Battle"), (0xAE12B0, "Unit-Upgrade"), (0xAE12B4, "Spell-Good"),
    (0xAE12B8, "Spell-Evil"), (0xAE12C0, "Battle-Fight"), (0xAE12C4, "Battle-Shoot"),
    (0xAE12C8, "Battle-Cure"), (0xAE12CC, "Battle-Bless"), (0xAE12D0, "Battle-Strike"),
    (0xAE12BC, "Battle-Sorcery"), (0xAE12D4, "Card-Move"), (0xAE12D8, "Item-Item"),
    (0xAE12DC, "Item-BlowWeapon"), (0xAE12E0, "Item-ShotWeapon"), (0xAE12E4, "Item-Armor"),
    (0xAE12E8, "Item-Helm"), (0xAE12EC, "Item-Shield"), (0xAE12F0, "Item-Staff"),
    (0xAE12F4, "Item-Amulet"), (0xAE12F8, "Item-Ring"), (0xAE12FC, "Item-Potion"),
    (0xAE12A8, "Item-Gold"), (0xC2A858, "Battle-Parry"),
]
SOUND_SLOTS = 0x5BD7FA      # 1024 × 0x2e: +0 group (1 music, 2 effects)
MUSIC_CURRENT = 0xAE129C    # the track playing (Music_Play, Music_NextRandom)
QUEUES = 0xB06F60           # 64 deferred-call queues of 0x404: count, then 64 × {fn, a, b, c}


def _sound_name(expr):
    table = json.dumps([[a, n] for a, n in SOUND_GLOBALS])
    return (f"(() => {{ const s = {expr}; for (const [ad, nm] of {table}) if (s && i32(ad) === s) "
            f"return nm; return null; }})()")


def _queue_entry(index):
    base = f"({QUEUES + 4} + a.q * 0x404 + ({index}) * 16)"
    return {"entry": f"[u32({base}), i32({base} + 4), i32({base} + 8), i32({base} + 12)]"}


def preset_av(arg=None):
    """Sounds, music and the timed animations (tools/difftest/av.py reads them):
    Sound_Play 0x481420 (slot, restart, loop; the slot's group and `_Sounds.ini` key),
    Music_Play 0x49d774 (track), Music_NextRandom 0x49d7f8 (the track it picked), and every
    entry pushed on the deferred-call queues (Queue_Push 0x48c2e4, Queue_PushFront 0x48c348:
    the callback and its {a, b, c}): battle slides 0x4afbd8, effects 0x4afe7c, passes 0x4afb54,
    card slides 0x4b0284, the walk 0x4ae6dc, waits 0x4ae280, camera glides 0x4af96c..."""
    return [
        {"name": "SoundPlay", "addr": 0x481420,
         "args": [["slot", "eax", "i32"], ["restart", "edx", "u8"], ["loop", "ecx", "u8"]],
         "enter": {"group": f"(a.slot > 0 && a.slot <= 1024) ? i32({SOUND_SLOTS} + a.slot * 0x2e) : null",
                   "name": _sound_name("a.slot")}, "ret": None},
        {"name": "MusicPlay", "addr": 0x49D774, "args": [["track", "eax", "i32"]],
         "enter": {"name": _sound_name("a.track")}, "ret": None},
        {"name": "MusicNext", "addr": 0x49D7F8, "args": [],
         "leave": {"name": _sound_name(f"i32({MUSIC_CURRENT})")}},
        {"name": "QueuePush", "addr": 0x48C2E4,
         "args": [["q", "eax", "i32"], ["fn", "edx", "hex"]],
         "leave": _queue_entry(f"i32({QUEUES} + a.q * 0x404) - 1")},
        {"name": "QueuePushFront", "addr": 0x48C348,
         "args": [["q", "eax", "i32"], ["fn", "edx", "hex"]], "leave": _queue_entry("0")},
    ]


PRESETS = {"random": preset_random, "ai": preset_ai, "ai_frames": preset_ai_frames,
           "advance": preset_advance, "damage": preset_damage, "events": preset_events,
           "av": preset_av}


def specs_for(names):
    """Hook specs for 'random,ai:1,events' or a path to a JSON list of specs."""
    out = []
    for part in [p.strip() for p in names.split(",") if p.strip()]:
        if part.endswith(".json") and os.path.exists(part):
            out += json.load(open(part))
            continue
        name, _, arg = part.partition(":")
        if name not in PRESETS:
            raise ValueError(f"unknown trace preset {name!r} (have {', '.join(PRESETS)})")
        out += PRESETS[name](arg or None)
    seen = {}
    for s in out:   # one hook per address (the later spec wins)
        seen[s["addr"]] = s
    return list(seen.values())


# --- the session ------------------------------------------------------------------------------
class Tracer:
    """Gadget + agent in a running game. `start()` after the game reached its main menu."""

    def __init__(self, mem, install_dir, presets, gadget=None, log=None):
        self.m = mem
        self.dir = install_dir
        self.specs = specs_for(presets) if isinstance(presets, str) else presets
        self.gadget = gadget
        self.log = log or (lambda *a: print(*a, file=sys.stderr))
        self.session = self.script = None
        self.dropped = 0

    def start(self, port=None):
        import frida
        port = port or free_port()
        put_gadget(self.dir, find_gadget(self.gadget), port)
        h = load_gadget(self.m)
        dev = frida.get_device_manager().add_remote_device(f"127.0.0.1:{port}")
        for i in range(50):
            try:
                self.session = dev.attach("Gadget")
                break
            except frida.ServerNotRunningError:
                time.sleep(0.1)
        else:
            raise RuntimeError("could not connect to the gadget")
        self.script = self.session.create_script(open(AGENT, encoding="utf-8").read())
        self.script.on("message", self._message)
        self.script.load()
        names = self.script.exports_sync.install(self.specs)
        info = self.script.exports_sync.info()
        self.log(f"frida gadget at {h:#x} ({info['arch']}, {info['platform']}); hooks: {', '.join(names)}")
        return self

    def _message(self, msg, data):
        if msg.get("type") == "error":
            self.log("trace agent error:", msg.get("description"), msg.get("stack", ""))

    def set_step(self, n):
        self.script.exports_sync.set_step(n)

    def drain(self):
        res = self.script.exports_sync.drain()
        self.dropped += res["dropped"]
        return res["records"], res["dropped"]

    def stop(self):
        try:
            if self.script:
                self.script.exports_sync.detach_all()
                self.script.unload()
            if self.session:
                self.session.detach()
        except Exception:
            pass
        self.script = self.session = None


def draws_from(records):
    """The Random records as the stub's draw tuples: (n, state before, return address, time)."""
    return [(r["a"]["n"], r["e"]["before"], r["ra"], r["t"]) for r in records if r["f"] == "Random"]


def steps_arg(text):
    """'9' or '8-10' or '8,12' → a set of steps."""
    out = set()
    for part in text.split(","):
        lo, _, hi = part.partition("-")
        out |= set(range(int(lo), int(hi or lo) + 1))
    return out


def show(path, steps=None, fns=None, army=None):
    """Print the records of a trace.jsonl, one line each, filtered."""
    from .memread import draw_site
    for line in open(path, encoding="utf-8"):
        r = json.loads(line)
        if (steps and r["s"] not in steps) or (fns and r["f"] not in fns):
            continue
        if army is not None and "k" in r.get("a", {}) and r["a"]["k"] != army:
            continue
        head = f"{r['s']:>3} t={r['t']:<7} {r['f']}"
        if r["f"] == "Random":
            print(f"{head}({r['a']['n']}) = {r.get('r')}  state {r['e']['before']}  "
                  f"from {r['ra']:#x} {draw_site(r['ra'])}")
            continue
        e, lv = r.get("e") or {}, r.get("l") or {}
        changed = {k: f"{e[k]}->{v}" for k, v in lv.items() if k in e and e[k] != v}
        rest = {k: v for k, v in lv.items() if k not in e}
        print(f"{head} {json.dumps(r.get('a', {}))}" + (f" = {r['r']}" if "r" in r else "") +
              (f"  in {json.dumps(e)}" if e else "") +
              (f"  changed {json.dumps(changed)}" if changed else "") +
              (f"  out {json.dumps(rest)}" if rest else ""))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--list", action="store_true", help="list the presets and their hooks (default)")
    ap.add_argument("--specs", metavar="PRESETS", help="print the hook specs of PRESETS as JSON")
    ap.add_argument("--read", metavar="TRACE", help="print a trace.jsonl, one record per line")
    ap.add_argument("--steps", type=steps_arg, help="with --read: steps, e.g. 9 or 8-10")
    ap.add_argument("--fn", help="with --read: hook names, comma-separated")
    ap.add_argument("--army", type=int, help="with --read: only this army's records (and the rest)")
    a = ap.parse_args(argv)
    if a.specs:
        print(json.dumps(specs_for(a.specs), indent=1))
        return
    if a.read:
        show(a.read, a.steps, set(a.fn.split(",")) if a.fn else None, a.army)
        return
    for name, fn in PRESETS.items():
        doc = " ".join((fn.__doc__ or "").split())
        hooks = ", ".join(f"{s['name']} {s['addr']:#x}" for s in fn(None))
        print(f"{name}: {hooks}" + (f"\n    {doc}" if doc else ""))


if __name__ == "__main__":
    main()
