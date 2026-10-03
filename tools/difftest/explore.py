"""A local LLM plays both games and looks for divergences (stage 5 of the diff test).

    ~/.local/opt/re-venv/bin/python -m tools.difftest.explore --hours 2 --max-new 3
    ~/.local/opt/re-venv/bin/python -m tools.difftest.explore --episodes 1 --maps РК1 --len 30

Per episode:
1. a map that New game can start (a standalone map or a campaign's first map, from the
   install) and a hero class are picked in turn, and a goal from a rotating list that pushes
   toward paths not tested yet (every building type, accept and decline offers, friendly
   armies, a weak army, noon and midnight, a long walk...);
2. an Ollama model (default qwen3.6) chooses the next few actions from a short text summary
   of Razdor's replay state (`razdor --replay` of the actions so far: the hero, the nearest
   buildings and armies with their cells, the window on screen and the text of the event
   that opened it, the actions valid there). Its JSON is repaired and checked; an action
   Razdor's replay skips (a note) is dropped and counted as invalid. Event messages that only
   need OK are closed without asking the model. The model only chooses actions, it never
   judges results;
3. the whole list is played on both sides by `run.py` (the Frida `random` trace on), and the
   differences of the step-local run are sorted by `known.py`: a known FINDINGS.md entry,
   downstream of one, the original's frame noise, or NEW;
4. for the first NEW one: the original is run once more on the prefix up to that step (with
   the `ai` and `events` traces too); if it disagrees with itself the difference is noise.
   Else the prefix is shrunk (chunks of actions dropped while the same field still differs
   as NEW, a few tries), and the repro, both states, both screenshots, the trace around the
   step and the classification go to `~/.cache/razdor-difftest/explore/<id>/`, and a short
   candidate entry is appended to `tools/difftest/CANDIDATES.md`.

`explore/log.jsonl` gets one line per episode (map, hero, goal, length, the model's calls and
invalid actions, the classes found, the times). The run stops after `--hours`, `--episodes`
or `--max-new` NEW candidates, whichever comes first.
"""

import argparse
import datetime
import glob
import json
import os
import random
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

from . import dtm, known
from .run import CACHE, PY, REPO, build, load_jsonl, razdor_shot

EXPLORE = os.path.join(CACHE, "explore")
RUNS = os.path.join(EXPLORE, "runs")
CANDIDATES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "CANDIDATES.md")
DEFAULT_INSTALL = os.path.expanduser("~/Games/Discord Times Community Update")

BUILDING_TYPES = {0: "palace", 1: "town", 2: "village", 3: "castle", 4: "fort", 5: "tavern",
                  6: "market", 7: "church", 8: "smithy/house", 9: "shipyard", 10: "altar",
                  11: "dungeon entrance", 12: "ruins", 13: "stone bridge", 14: "wooden bridge",
                  15: "obelisk"}
FACTIONS = {1: "player's", 2: "ally", 3: "neighbour", 4: "enemy"}
EVENT_TYPES = {1: "global", 2: "local", 3: "quest", 4: "rumour"}

GOALS = [
    "Visit building types you have not visited yet (see the list 'not visited yet'): walk onto them.",
    "Accept offers: go to villages, towns and other buildings and answer YES to every question.",
    "Decline offers: go to villages, towns and other buildings and answer NO to every question.",
    "Talk to a friendly army (ally or neighbour): walk onto its cell.",
    "Fight a weak hostile army (the one with the least total HP): walk onto it, and in the "
    "battle attack its units with battle_act on side 2.",
    "Wait through noon and midnight: use wait with hours 4 several times and close the reports.",
    "Walk far: pick buildings far from the hero, on the other side of the map, and walk there.",
    "Find magic: visit churches, altars, obelisks and towns (places that sell or teach spells).",
    "Trade: visit markets, towns and villages (places that buy and sell goods).",
    "Hire troops: visit castles, forts, towns and taverns (places with barracks).",
    "Quests: read the event messages, answer YES to quests, and go where the messages point.",
    "Shop: in a market tab buy an item you can afford (buy), later sell one (sell).",
    "Recruit: in a building with barracks hire units you can afford (hire).",
    "Care: after a fight heal wounded units (heal) or raise dead ones (resurrect) where offered.",
    "Magic: learn a spell in a sanctuary (learn), then cast spells from your book (cast).",
    "Gear: put items from your pack on your units (equip), the hero first.",
    "Villages: visit villages and answer YES to what the villagers offer.",
]

SYSTEM = """You are a game tester playing the fantasy strategy game Discord Times, through a tiny
action language. You get the current state as text and a goal. Reply with JSON only:
{"actions": [ ... 1 to 4 actions ... ], "why": "a few words"}
Actions (use only the ones listed as valid for the current screen):
  {"op":"click_map","x":X,"y":Y}   walk to the cell (X,Y); a building's cell enters it, an
                                    army's cell meets that army (a fight with an enemy)
  {"op":"wait","hours":1}  or  {"op":"wait","hours":4}   let time pass on the world map
  {"op":"ok"}                       close the message or building window on screen
  {"op":"answer","yes":true}  or  {"op":"answer","yes":false}   answer the question on screen
  {"op":"battle_act","side":2,"row":R,"col":C}   in battle: act on that card (side 2 = enemy,
                                    side 1 = own); rows 1 front, 2 back, 3 reserve; cols 1-6
  {"op":"battle_pass"}              in battle: skip the turn of the unit that acts
In a building window (rows and units as listed in the state, 0-based):
  {"op":"buy","slot":N}  {"op":"sell","slot":N}   buy row N of the goods / sell row N of the sell list
  {"op":"hire","slot":N}                          hire a unit of barracks slot N
  {"op":"heal","unit":N}  {"op":"resurrect","unit":N}   heal / raise unit N of your army
  {"op":"learn","slot":N}                         learn the spell of row N
On the map (also from a building window):
  {"op":"cast","slot":N}  or  {"op":"cast","slot":N,"army":ID}   cast book entry N (ID: target army
                                    for a spell on enemies)
  {"op":"equip","slot":N,"unit":U}  put pack item N on unit U (0 = the hero)
Coordinates are map cells. Prefer cells listed in the state (buildings, armies). Do not attack
armies much stronger than yours."""


# --- the map file: names, factions, events ------------------------------------------------------
class MapInfo:
    def __init__(self, path):
        self.path = path
        self.file = os.path.basename(path)
        self.stem = os.path.splitext(self.file)[0]
        self.m = dtm.read(path)
        d = dtm._payload(path)
        u32 = lambda o: struct.unpack_from("<I", d, o)[0]
        sizes = [u32(o) for o in (0x1C, 0x20, 0x24, 0x28, 0x2C, 0x30)]
        nb, na, ne = len(self.m.buildings), len(self.m.armies), self.m.event_count
        off_armies = dtm.HEADER + sum(sizes[:3])
        off_events = dtm.HEADER + sum(sizes[:5])
        self.army_faction = {i + 1: d[off_armies + i * dtm.ARMY + 64] for i in range(na)}
        self.army_patrols = {i + 1: d[off_armies + i * dtm.ARMY + 60] for i in range(na)}
        ev = [d[off_events + i * dtm.EVENT: off_events + (i + 1) * dtm.EVENT] for i in range(ne)]
        strings = d[u32(0x18):].split(b"\0", 4 + 3 * (nb + na + ne))
        s = [x.decode("cp1251", "replace") for x in strings[: 4 + 3 * (nb + na + ne)]]
        s += [""] * (4 + 3 * (nb + na + ne) - len(s))
        self.building_name = {i + 1: s[4 + 3 * i] for i in range(nb)}
        o = 4 + 3 * nb
        self.army_name = {i + 1: s[o + 3 * i] for i in range(na)}
        o += 3 * na
        self.events = {}
        for i in range(ne):
            title = s[o + 3 * i].split("%")[0]
            self.events[i + 1] = {"type": ev[i][1], "question": ev[i][76] != 0, "title": title,
                                  "ask": s[o + 3 * i + 1], "text": s[o + 3 * i + 2]}
        self.buildings = {b.id: b for b in self.m.buildings}

    def heroes(self):
        """The hero classes with a start preset (1 knight, 2 archmage, 3 ranger)."""
        return [k + 1 for k, p in enumerate(self.m.presets) if (p.x, p.y) != (0, 0)]

    def building_at(self, x, y):
        for b in self.m.buildings:
            if b.x - max(b.size_x, 1) < x <= b.x and b.y - max(b.size_y, 1) < y <= b.y:
                return b
        return None

    def known_buildings(self):
        return {b.id: (b.type, b.x, b.y, b.size_x, b.size_y) for b in self.m.buildings}


def startable_maps(install):
    """Maps New game can start: standalone maps and the first map of a campaign (kind 0, 1)."""
    out = []
    for p in sorted(glob.glob(os.path.join(install, "Maps_Rus", "*.DTm"))):
        try:
            m = MapInfo(p)
        except Exception as e:  # a map we cannot read is just not explored
            print(f"skip {p}: {e}", file=sys.stderr)
            continue
        if m.m.kind != 2:
            out.append(m)
    return out


# --- Razdor's replay ------------------------------------------------------------------------------
class Razdor:
    def __init__(self, exe, work):
        self.exe = exe
        self.work = work
        os.makedirs(work, exist_ok=True)

    def replay(self, actions, look=False):
        """(states, notes by step, ok)."""
        fd, path = tempfile.mkstemp(suffix=".jsonl", dir=self.work)
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            for a in actions:
                f.write(json.dumps(a, ensure_ascii=False) + "\n")
        try:
            res = subprocess.run([self.exe, "--replay", path] + (["--look"] if look else []), cwd=REPO, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, text=True, timeout=120)
        finally:
            os.unlink(path)
        states = [json.loads(l) for l in res.stdout.splitlines() if l.strip().startswith("{")]
        notes = {}
        for l in res.stderr.splitlines():
            m = re.match(r"note: step (\d+): (.*)", l)
            if m:
                notes.setdefault(int(m.group(1)), []).append(m.group(2))
        return states, notes, res.returncode == 0

    def look(self, actions):
        """(state, screen, look) after `actions`: Razdor's own account of its screen
        (`--look`): 'map', 'building', 'dialog' (a message OK closes), 'question', 'offer' (a
        village's yes/no question), 'battle' or 'ended'; `look` also lists what the building
        window, the book and the pack offer."""
        n = len(actions)
        states, _, ok = self.replay(actions, look=True)
        if not ok or len(states) < n:
            return None, "ended", {}
        st = states[n - 1]
        lk = st.pop("look", {}) or {}
        return st, lk.get("screen", "map"), lk

    def reachable(self, actions, cells, workers=8):
        """The cells of `cells` a click_map accepts now (explored, a way there)."""
        from concurrent.futures import ThreadPoolExecutor
        n = len(actions)

        def ok(c):
            _, notes, good = self.replay(actions + [{"op": "click_map", "x": c[0], "y": c[1]}])
            return good and not notes.get(n)
        cells = list(dict.fromkeys(cells))
        with ThreadPoolExecutor(workers) as ex:
            return {c for c, good in zip(cells, ex.map(ok, cells)) if good}


DIRS = {"N": (0, -1), "NE": (1, -1), "E": (1, 0), "SE": (1, 1), "S": (0, 1), "SW": (-1, 1),
        "W": (-1, 0), "NW": (-1, -1)}


def sample_cells(info, hp):
    """Cells around the hero in 8 directions at 4..20 cells."""
    out = {}
    for name, (dx, dy) in DIRS.items():
        for r in (4, 8, 12, 16, 20):
            x, y = hp[0] + dx * r, hp[1] + dy * r
            if 0 <= x < info.m.width and 0 <= y < info.m.height:
                out[(x, y)] = name
    return out


VALID = {"map": ("click_map", "wait"), "building": ("ok", "click_map", "wait"),
         "dialog": ("ok",), "question": ("answer",), "offer": ("answer",),
         "battle": ("battle_act", "battle_pass"), "ended": ()}
SERVICE_OPS = ("buy", "sell", "hire", "heal", "resurrect", "learn", "cast", "equip")


def valid_ops(screen, look):
    """The ops that make sense on `screen` with what Razdor's `look` lists."""
    ops = list(VALID[screen])
    if screen in ("map", "building"):
        if look.get("book"):
            ops.append("cast")
        if look.get("pack"):
            ops.append("equip")
    if screen == "building":
        if look.get("goods"):
            ops.append("buy")
        if look.get("sell"):
            ops.append("sell")
        if look.get("recruits"):
            ops.append("hire")
        if any(x["op"] == "heal" for x in look.get("services", [])):
            ops.append("heal")
        if any(x["op"] == "resurrect" for x in look.get("services", [])):
            ops.append("resurrect")
        if any(not x["known"] for x in look.get("spells", [])):
            ops.append("learn")
    return tuple(ops)


def strength(units):
    """A rough army strength: the hit points of its living units."""
    return sum(max(0, u.get("hp", 0)) for u in units or [])


# --- the summary for the model -------------------------------------------------------------------
def dist(a, b):
    return max(abs(a[0] - b[0]), abs(a[1] - b[1]))


def clock_text(clock):
    day, mins = divmod(clock, 1440)
    return f"day {day % 365 + 1}, {mins // 60:02d}:{mins % 60:02d}"


def summarise(info, state, screen, goal, visited_types, history, new_events, rejected, reach=None,
              look=None, orig_view=None):
    """`reach`: (set of cells click_map accepts, {sample cell: direction}) or None; `look`:
    Razdor's account of the screen (`--look`); `orig_view`: what the original shows when it
    differs from Razdor's screen."""
    look = look or {}
    hero = state["hero"]
    hp = (hero["x"], hero["y"])
    L = [f"Map {info.stem} ({info.m.width}x{info.m.height} cells). {clock_text(state['clock'])}.",
         f"GOAL: {goal}",
         f"Hero at ({hp[0]},{hp[1]}), gold {hero['gold']}, mana {hero['mana']}, "
         f"{len(hero['units'])} units, total HP {sum(u['hp'] for u in hero['units'])}."]
    for e in new_events[-2:]:
        ev = info.events.get(e)
        if ev and (ev["text"] or ev["ask"]):
            kind = EVENT_TYPES.get(ev["type"], "?")
            L.append(f"Event {e} ({kind}) just fired: \"{(ev['ask'] or ev['text'])[:300]}\"")
    desc = {"map": "the world map", "building": "a building window (ok closes it; click_map/wait also close it)",
            "dialog": "a message (ok closes it)", "question": "a YES/NO question (answer it)",
            "offer": "a village's offer, a YES/NO question (answer it)",
            "battle": "a BATTLE", "ended": "the game is over"}[screen]
    L.append(f"Screen: {desc}. Valid ops now: {', '.join(valid_ops(screen, look))}.")
    if orig_view:
        L.append(f"WARNING: the original game shows {orig_view} while Razdor shows {screen}: "
                 "prefer actions that close windows (ok / answer) until both agree.")
    if screen == "building":
        b = look.get("building") or {}
        L.append(f"Building {b.get('id')} ({b.get('kind')}), tabs: {', '.join(look.get('tabs', []))}.")
        if look.get("goods"):
            L.append("  goods (buy slot): " + "; ".join(f"{g['slot']}: {g['name']} {g['price']}g" for g in look["goods"][:12]))
        if look.get("sell"):
            L.append("  sell list (sell slot): " + "; ".join(f"{g['slot']}: item {g['id']} for {g['price']}g" for g in look["sell"][:12]))
        if look.get("recruits"):
            L.append("  barracks (hire slot): " + "; ".join(f"{r['slot']}: {r['name']} {r['price']}" for r in look["recruits"]))
        if look.get("services"):
            L.append("  services: " + "; ".join(f"{x['op']} unit {x['unit']} for {x['price']}" for x in look["services"]))
        if look.get("spells"):
            L.append("  spells (learn slot): " + "; ".join(f"{x['slot']}: {x['name']} {x['price']}g" + (" (known)" if x["known"] else "") for x in look["spells"]))
    if screen in ("map", "building"):
        if look.get("book"):
            L.append("Spell book (cast slot): " + "; ".join(f"{x['slot']}: {x['name']} {x['mana']} mana" + (" (needs army)" if x["enemy"] else "") for x in look["book"]))
        if look.get("pack"):
            L.append("Pack (equip slot): " + "; ".join(f"{x['slot']}: {x['name']}" for x in look["pack"][:12]))
        L.append("Your units: " + "; ".join(f"{i}: type {u['type']} hp {u['hp']}" for i, u in enumerate(hero["units"])))
    if screen == "battle":
        b = state["battle"]
        L.append(f"Battle turn {b.get('turn')}, acting card: {b.get('actor')} (side,row,col).")
        for side, name in ((0, "own (side 1)"), (1, "enemy (side 2)")):
            us = b["sides"][side]
            L.append(f"  {name}: " + "; ".join(
                f"row {u['row']} col {u['col']} type {u['type']} hp {u['hp']}" for u in us))
    else:
        owners = {b["id"]: b.get("owner") for b in state.get("buildings", [])}
        bs = sorted(info.m.buildings, key=lambda b: dist(hp, (b.x, b.y)))
        ok_cells, samples = reach or (None, {})
        tag = lambda c: "" if ok_cells is None else " REACHABLE" if c in ok_cells else " (unexplored or no way: not clickable yet)"
        L.append("Buildings (nearest first; click their cell to enter):")
        for b in bs[:10]:
            own = owners.get(b.id)
            ow = "yours" if own == 0 else "no owner" if own in (255, None) else f"army {own}'s"
            L.append(f"  building {b.id} {BUILDING_TYPES.get(b.type, b.type)} at ({b.x},{b.y}), "
                     f"distance {dist(hp, (b.x, b.y))}, {ow}" + (" [visited]" if b.id in visited_types.get('ids', ()) else "")
                     + tag((b.x, b.y)))
        if samples and ok_cells is not None:
            far = {}
            for c, d in samples.items():
                if c in ok_cells and dist(hp, c) > dist(hp, far.get(d, hp)):
                    far[d] = c
            if far:
                L.append("Open cells you can walk to (farthest per direction; walking explores the map): " +
                         ", ".join(f"{d} ({c[0]},{c[1]})" for d, c in far.items()))
        arm = [a for a in state.get("armies", []) if a.get("active", True) and a.get("alive", True) and "x" in a]
        arm.sort(key=lambda a: dist(hp, (a["x"], a["y"])))
        if arm:
            L.append("Armies on the map (nearest first; click their cell to meet them):")
        mine = strength(hero["units"])
        for a in arm[:8]:
            us = a.get("units", [])
            st_ = strength(us)
            risk = " TOO STRONG, avoid" if st_ > mine * STRONGER else ""
            L.append(f"  army {a['id']} {FACTIONS.get(info.army_faction.get(a['id']), '?')} at "
                     f"({a['x']},{a['y']}), distance {dist(hp, (a['x'], a['y']))}, {len(us)} units, "
                     f"total HP {st_}{risk}" + tag((a["x"], a["y"])))
        types = {b.type for b in info.m.buildings} - visited_types.get("types", set()) - {13, 14}
        if types:
            L.append("Building types not visited yet: " + ", ".join(sorted(BUILDING_TYPES.get(t, str(t)) for t in types)))
    if history:
        L.append("Last actions: " + "; ".join(history[-6:]))
    if rejected:
        L.append("REJECTED (not possible now, do not repeat): " + "; ".join(rejected[-4:]))
    L.append('Reply with JSON {"actions":[...],"why":"..."}.')
    return "\n".join(L)


# A hostile army whose hit points pass the hero's by this factor is not attacked.
STRONGER = 1.3


# --- the model ------------------------------------------------------------------------------------
class Model:
    def __init__(self, url, model, temperature=0.8, timeout=600):
        self.url = url.rstrip("/")
        self.model = model
        self.temperature = temperature
        self.timeout = timeout
        self.calls = 0
        self.seconds = 0.0

    def _post(self, path, body, timeout):
        req = urllib.request.Request(self.url + path, data=json.dumps(body).encode(),
                                     headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read())

    def available(self, tries=5, pause=30):
        for i in range(tries):
            try:
                tags = self._post("/api/show", {"model": self.model}, 30)
                return bool(tags)
            except (urllib.error.URLError, OSError, ValueError) as e:
                print(f"ollama not answering ({e}); retry {i + 1}/{tries} in {pause}s", file=sys.stderr)
                time.sleep(pause)
        return False

    def ask(self, prompt):
        body = {"model": self.model, "stream": False, "think": False, "format": "json",
                "keep_alive": "2h", "options": {"temperature": self.temperature, "num_ctx": 8192},
                "messages": [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}]}
        t0 = time.time()
        for i in range(4):
            try:
                res = self._post("/api/chat", body, self.timeout)
                self.calls += 1
                self.seconds += time.time() - t0
                return res.get("message", {}).get("content", "")
            except (urllib.error.URLError, OSError, ValueError) as e:
                print(f"ollama call failed ({e}); retry in 20s", file=sys.stderr)
                time.sleep(20)
        raise RuntimeError("ollama did not answer 4 times in a row")


# --- repairing the model's JSON -----------------------------------------------------------------
OP_ALIASES = {"click": "click_map", "move": "click_map", "walk": "click_map", "go": "click_map",
              "goto": "click_map", "move_to": "click_map", "attack": "click_map", "visit": "click_map",
              "enter": "click_map", "rest": "wait", "close": "ok", "confirm": "ok", "continue": "ok",
              "act": "battle_act", "strike": "battle_act", "shoot": "battle_act", "pass": "battle_pass",
              "skip": "battle_pass", "accept": "answer", "decline": "answer", "yes": "answer", "no": "answer",
              "purchase": "buy", "recruit": "hire", "cure": "heal", "raise": "resurrect", "revive": "resurrect",
              "study": "learn", "spell": "cast", "wear": "equip", "use": "equip"}


def _to_int(v):
    if isinstance(v, bool):
        return None
    if isinstance(v, (int, float)):
        return int(v)
    if isinstance(v, str) and re.fullmatch(r"\s*-?\d+(\.\d+)?\s*", v):
        return int(float(v))
    return None


def _to_bool(v):
    if isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return v != 0
    if isinstance(v, str):
        s = v.strip().lower()
        if s in ("true", "yes", "y", "1", "accept", "да"):
            return True
        if s in ("false", "no", "n", "0", "decline", "нет"):
            return False
    return None


def parse_json(text):
    """The first JSON value in the model's text (fences, <think> blocks and trailing junk
    removed). Returns (value, repaired)."""
    t = re.sub(r"<think>.*?</think>", "", text or "", flags=re.S).strip()
    t = re.sub(r"^```(?:json)?|```$", "", t, flags=re.M).strip()
    try:
        return json.loads(t), False
    except ValueError:
        pass
    for opener, closer in (("{", "}"), ("[", "]")):
        i, j = t.find(opener), t.rfind(closer)
        if i >= 0 and j > i:
            chunk = t[i:j + 1]
            for fix in (chunk, re.sub(r",\s*([}\]])", r"\1", chunk), chunk.replace("'", '"')):
                try:
                    return json.loads(fix), True
                except ValueError:
                    continue
    objs = []
    for m in re.finditer(r"\{[^{}]*\}", t):
        try:
            objs.append(json.loads(m.group(0)))
        except ValueError:
            pass
    return (objs, True) if objs else (None, True)


def normalise_action(a):
    """One action of the vocabulary, or None."""
    if isinstance(a, str):
        a = {"op": a}
    if not isinstance(a, dict):
        return None
    op = str(a.get("op") or a.get("action") or a.get("type") or "").strip().lower()
    raw_op = op
    op = OP_ALIASES.get(op, op)
    if op == "click_map":
        x, y = _to_int(a.get("x")), _to_int(a.get("y"))
        cell = a.get("cell") or a.get("target") or a.get("pos")
        if (x is None or y is None) and isinstance(cell, (list, tuple)) and len(cell) == 2:
            x, y = _to_int(cell[0]), _to_int(cell[1])
        if x is None or y is None or x < 0 or y < 0:
            return None
        return {"op": "click_map", "x": x, "y": y}
    if op == "wait":
        h = _to_int(a.get("hours", 1))
        return {"op": "wait", "hours": 1 if h is None or h <= 2 else 4}
    if op == "ok":
        return {"op": "ok"}
    if op == "answer":
        y = _to_bool(a.get("yes", a.get("answer", a.get("value"))))
        if y is None and raw_op in ("accept", "yes", "decline", "no"):
            y = raw_op in ("accept", "yes")
        return None if y is None else {"op": "answer", "yes": y}
    if op == "battle_act":
        side, row, col = _to_int(a.get("side", 2)), _to_int(a.get("row")), _to_int(a.get("col"))
        if side not in (1, 2) or row not in (1, 2, 3) or col is None or not 1 <= col <= 6:
            return None
        return {"op": "battle_act", "side": side, "row": row, "col": col}
    if op == "battle_pass":
        return {"op": "battle_pass"}
    if op in ("buy", "sell", "hire", "learn"):
        k = _to_int(a.get("slot", a.get("row", a.get("index"))))
        return None if k is None or k < 0 else {"op": op, "slot": k}
    if op in ("heal", "resurrect"):
        k = _to_int(a.get("unit", a.get("slot")))
        return None if k is None or k < 0 else {"op": op, "unit": k}
    if op == "cast":
        k = _to_int(a.get("slot", a.get("spell")))
        if k is None or k < 0:
            return None
        army = _to_int(a.get("army", a.get("target")))
        return {"op": "cast", "slot": k} | ({"army": army} if army is not None else {})
    if op == "equip":
        k, u = _to_int(a.get("slot", a.get("item"))), _to_int(a.get("unit", 0))
        return None if k is None or k < 0 or u is None or u < 0 else {"op": "equip", "slot": k, "unit": u}
    return None  # new_game, battle_auto (a no-op in the original), keys, unknown ops


def repair(text):
    """(actions, stats): the model's actions in the vocabulary, and counts of what was fixed
    ('repaired': the JSON needed fixing, 'dropped': items that are no action)."""
    v, repaired = parse_json(text)
    if isinstance(v, dict):
        items = v.get("actions") if isinstance(v.get("actions"), list) else \
            [v["action"]] if isinstance(v.get("action"), dict) else [v] if "op" in v else []
    elif isinstance(v, list):
        items = v
    else:
        items = []
    acts = [normalise_action(a) for a in items[:6]]
    out = [a for a in acts if a]
    return out, {"repaired": int(repaired), "unparsed": int(v is None),
                 "dropped": len(acts) - len(out)}


# --- playing an episode --------------------------------------------------------------------------
def act_text(a):
    rest = " ".join(str(v).lower() if isinstance(v, bool) else str(v) for k, v in a.items() if k != "op")
    return a["op"] + (" " + rest if rest else "")


def fallback(info, state, screen, rnd, visited, razdor=None, acts=None, avoid=()):
    """An action when the model gave nothing usable."""
    if screen in ("dialog", "building"):
        return {"op": "ok"}
    if screen in ("question", "offer"):
        return {"op": "answer", "yes": rnd.random() < 0.5}
    if screen == "battle":
        # A press on a foe Razdor takes (a melee unit cannot reach every card), else a pass.
        foes = list(state["battle"]["sides"][1])
        rnd.shuffle(foes)
        for u in foes:
            a = {"op": "battle_act", "side": 2, "row": u["row"], "col": u["col"]}
            if razdor is None or not razdor.replay(acts + [a])[1].get(len(acts)):
                return a
        return {"op": "battle_pass"}
    hp = (state["hero"]["x"], state["hero"]["y"])
    bs = [(b.x, b.y) for b in info.m.buildings if b.id not in visited.get("ids", ())]
    cells = [c for c in bs + list(sample_cells(info, hp)) if c not in avoid]
    ok = sorted(razdor.reachable(acts, cells)) if razdor else []
    if ok:
        good_b = [c for c in ok if c in bs]
        c = rnd.choice(good_b or ok)
        return {"op": "click_map", "x": c[0], "y": c[1]}
    return {"op": "wait", "hours": 1}


# --- the original, played along -------------------------------------------------------------
ORIGINAL_SCREENS = {"world": "map", "building": "building", "village": "building", "shipyard": "building",
                    "event": "dialog", "question": "question", "battle": "battle", "exit_battle": "battle",
                    "main_menu": "ended"}


class LiveOriginal:
    """The original played along with the episode, action by action, recorded as
    `original.py` records a run (states, shots, run log, Frida `random` trace), so that the
    explorer sees its screen and `run.py --reuse-original` diffs the episode without playing
    the original again."""

    def __init__(self, out, trace="random"):
        self.out, self.trace = out, trace
        self.o = None
        self.failed = None
        self.n = 0

    def start(self):
        from . import original
        os.makedirs(self.out, exist_ok=True)
        for name in ("run.jsonl", "original.jsonl", "trace.jsonl"):
            open(os.path.join(self.out, name), "w").close()
        self.o = original.Original(trace=True, hold_music=True, frida=self.trace,
                                   log=lambda *a: print("  original:", *a, file=sys.stderr))
        self.o.start()
        self.n = 0

    def do(self, act):
        """Plays `act` as step self.n. Returns its note (None: applied)."""
        from . import original
        if self.failed:
            return "harness down"
        o = self.o
        try:
            if o.tracer:
                o.tracer.set_step(self.n)
            try:
                note = o.perform(act)
            except original.NotApplicable as e:
                note = f"skipped: {e}"
                o.settle(quiet=0.3)
            o.record(self.out, self.n, act, note)
        except Exception as e:  # HarnessError, a dead process...
            self.failed = f"step {self.n}: {e}"
            print(f"  original failed at {self.failed}", file=sys.stderr)
            self.stop()
            return "harness down"
        self.n += 1
        return note

    def view(self, info):
        """(screen in the explorer's words, a description) of what the original shows."""
        if self.failed or not self.o:
            return None, ""
        g = self.o.game
        try:
            scr = g.screen()
            mine = ORIGINAL_SCREENS.get(scr, scr)
            text = scr
            if scr == "event":
                from . import memread
                hidden, *_rest = g.widget(memread.EVENT_YES)
                w = g.widget(memread.EVENT_YES)[3]
                if not hidden and w > 0:
                    mine = "question"
                ev = g.dialog_event() + 1
                e = info.events.get(ev)
                text = f"an event window (event {ev}" + (f", {e['title'][:60]}" if e else "") + ")"
                if ev > len(info.events):
                    text = "an event window (a report or a village offer)"
            return mine, text
        except Exception as e:
            return None, f"unreadable ({e})"

    def replay(self, acts):
        """Starts again and plays `acts` (a rewind to an earlier point of the episode)."""
        self.stop()
        self.failed = None
        try:
            self.start()
        except Exception as e:
            self.failed = f"restart: {e}"
            return
        for a in acts:
            self.do(a)

    def stop(self):
        if self.o:
            try:
                self.o.stop()
            except Exception:
                pass
            self.o = None


def battle_start(screens, upto):
    """The index of the action that opened the battle going on at `upto` (screens[i] is the
    screen after action i)."""
    i = upto
    while i > 0 and screens[i - 1] == "battle":
        i -= 1
    return i


def play_episode(info, hero, goals, razdor, model, length, rnd, log, live=None, max_rewinds=3):
    acts = [{"op": "new_game", "map": info.stem, "hero": hero}]
    st = {"model_calls": 0, "proposed": 0, "repaired_json": 0, "unparsed": 0, "dropped": 0,
          "wrong_screen": 0, "rejected": 0, "too_strong": 0, "fallbacks": 0, "auto_ok": 0,
          "accepted_from_model": 0, "rewinds": 0, "resyncs": 0, "screen_mismatch": 0,
          "ops_razdor": {}, "ops_original": {}}
    visited = {"types": set(), "ids": set()}
    history, rejected, empty_rounds = [], [], 0
    seen_events = set()
    new_events = []
    avoid = set()          # cells of armies that beat the hero (rewound)
    state, screen, look = razdor.look(acts)
    if state is None:
        return acts, st, "razdor could not start the map"
    screens = [screen]
    if live:
        try:
            live.start()
            live.do(acts[0])
        except Exception as e:
            live.failed = f"start: {e}"
    seen_events |= set(state.get("events_done", []))
    goal_i = 0

    def count(a, o_note):
        st["ops_razdor"][a["op"]] = st["ops_razdor"].get(a["op"], 0) + 1
        if live and not live.failed and o_note is None:
            st["ops_original"][a["op"]] = st["ops_original"].get(a["op"], 0) + 1

    def push(a, tag=""):
        """Plays `a` on both sides; returns the original's note."""
        nonlocal state, screen, look, new_events
        acts.append(a)
        o_note = live.do(a) if live else None
        count(a, o_note)
        state, screen, look = razdor.look(acts)
        if state is None:
            screen = "ended"
            screens.append(screen)
            return o_note
        screens.append(screen)
        done = set(state.get("events_done", []))
        fresh = sorted(done - seen_events)
        seen_events.update(done)
        new_events = fresh
        history.append(act_text(a) + tag + (f" -> events {fresh}" if fresh else "") + f" -> {screen}"
                       + (f" (original: {o_note})" if o_note else ""))
        return o_note

    while len(acts) < length:
        goal = goals[(goal_i + (len(acts) // 20)) % len(goals)]
        if screen == "ended":
            # The game ended in a battle (lost): rewind the script to before the action that
            # opened it, as a reload of the last point before the battle, and go on elsewhere.
            if len(screens) > 1 and screens[-2] == "battle" and st["rewinds"] < max_rewinds:
                k = battle_start(screens, len(screens) - 2)
                opener = acts[k] if k < len(acts) else {}
                if opener.get("op") == "click_map":
                    avoid.add((opener["x"], opener["y"]))
                del acts[k:]
                del screens[k:]
                st["rewinds"] += 1
                state, screen, look = razdor.look(acts)
                if live:
                    live.replay(acts)
                history.append(f"(rewound to before the lost battle of step {k})")
                rejected.append(f"click_map {opener.get('x')} {opener.get('y')}: that army beat you, avoid it")
                continue
            break
        # The original's view, when it shows another screen.
        o_screen, o_text = live.view(info) if live else (None, "")
        orig_view = None
        if o_screen and o_screen != screen and not (o_screen == "dialog" and screen == "question"):
            st["screen_mismatch"] += 1
            orig_view = f"{o_text} (screen {o_screen})"
            # A window only the original shows: close it (Razdor notes the step and skips it).
            if o_screen in ("dialog",) and screen in ("map", "building") and st["resyncs"] < 40:
                st["resyncs"] += 1
                push({"op": "ok"}, " (resync)")
                continue
            if o_screen == "question" and screen in ("map", "building") and st["resyncs"] < 40:
                st["resyncs"] += 1
                push({"op": "answer", "yes": False}, " (resync)")
                continue
            if o_screen in ("map", "building") and screen == "dialog" and st["resyncs"] < 40:
                st["resyncs"] += 1
                push({"op": "ok"}, " (resync)")
                continue
        if screen == "dialog":
            cand_list = [{"op": "ok"}]
            st["auto_ok"] += 1
            from_model = False
        else:
            reach = None
            if screen in ("map", "building"):
                hp = (state["hero"]["x"], state["hero"]["y"])
                samples = sample_cells(info, hp)
                cells = list(samples) + [(b.x, b.y) for b in info.m.buildings if dist(hp, (b.x, b.y)) <= 30][:16] + \
                    [(x["x"], x["y"]) for x in state.get("armies", []) if "x" in x and x.get("active", True)
                     and x.get("alive", True) and dist(hp, (x["x"], x["y"])) <= 30][:10]
                reach = (razdor.reachable(acts, cells), samples)
            prompt = summarise(info, state, screen, goal, visited, history, new_events, rejected, reach,
                               look, orig_view)
            text = model.ask(prompt)
            st["model_calls"] += 1
            cand_list, fix = repair(text)
            st["repaired_json"] += fix["repaired"]
            st["unparsed"] += fix["unparsed"]
            st["dropped"] += fix["dropped"]
            st["proposed"] += len(cand_list) + fix["dropped"]
            from_model = True
            log({"prompt_screen": screen, "orig_view": orig_view, "reply": text[:400]})
        took = 0
        for a in cand_list:
            if a["op"] not in valid_ops(screen, look):
                st["wrong_screen"] += 1
                rejected.append(f"{act_text(a)} (not valid on {screen})")
                break
            if a["op"] == "click_map":
                c = (a["x"], a["y"])
                foe = next((x for x in state.get("armies", []) if (x.get("x"), x.get("y")) == c
                            and x.get("active", True) and x.get("alive", True)), None)
                mine = strength(state["hero"]["units"])
                friendly = foe and info.army_faction.get(foe["id"]) in (1, 2)
                if c in avoid or (foe and not friendly and strength(foe.get("units")) > mine * STRONGER):
                    st["too_strong"] += 1
                    rejected.append(f"{act_text(a)}: that army is too strong for you")
                    break
            _, notes, _ = razdor.replay(acts + [a])
            n = notes.get(len(acts))
            if n:
                st["rejected"] += 1
                rejected.append(f"{act_text(a)}: {n[0].split(': ', 1)[-1]}")
                break
            took += 1
            if from_model:
                st["accepted_from_model"] += 1
            if a["op"] == "click_map":
                b = info.building_at(a["x"], a["y"])
                if b:
                    visited["types"].add(b.type)
                    visited["ids"].add(b.id)
            push(a)
            if screen != "map" or len(acts) >= length:
                break  # the screen changed: ask again with the new state
        if took == 0:
            empty_rounds += 1
            if empty_rounds >= 3:
                a = fallback(info, state, screen, rnd, visited, razdor, acts, avoid)
                _, notes, _ = razdor.replay(acts + [a])
                if notes.get(len(acts)):
                    a = {"op": "wait", "hours": 1} if screen == "map" else {"op": "ok"}
                    _, notes, _ = razdor.replay(acts + [a])
                    if notes.get(len(acts)):
                        return acts, st, f"stuck on {screen}"
                st["fallbacks"] += 1
                empty_rounds = 0
                push(a, " (fallback)")
        else:
            empty_rounds = 0
            rejected = rejected[-2:]
    return acts, st, "length reached" if len(acts) >= length else f"screen {screen}"


# --- diffing and classifying ---------------------------------------------------------------------
def run_both(actions, name, trace="random", shots=False, reuse=None):
    """run.py on `actions` into RUNS/<name>; returns the run folder. `reuse`: an original's
    run folder recorded along the episode (`LiveOriginal`)."""
    os.makedirs(RUNS, exist_ok=True)
    path = os.path.join(RUNS, name + ".jsonl")
    with open(path, "w", encoding="utf-8") as f:
        for a in actions:
            f.write(json.dumps(a, ensure_ascii=False) + "\n")
    cmd = [PY, "-m", "tools.difftest.run", "--actions", path, "--name", name, "--runs", RUNS,
           "--no-build", "--trace", trace]
    if not shots:
        cmd.append("--no-shots")
    if reuse:
        cmd += ["--reuse-original", reuse]
    t0 = time.time()
    with open(os.path.join(RUNS, name + ".log"), "w") as logf:
        subprocess.run(cmd, cwd=REPO, stdout=logf, stderr=subprocess.STDOUT)
    print(f"  run {name}: {time.time() - t0:.0f}s", file=sys.stderr)
    return os.path.join(RUNS, name)


def classify_run(run_dir, info):
    """(per_step classes of the step-local run, the diff, the context)."""
    p = os.path.join(run_dir, "diff.json")
    if not os.path.exists(p):
        return None, None, None
    diff = json.load(open(p, encoding="utf-8"))
    acts = [r["action"] for r in diff["local"]]
    ctx = known.Context(acts, info.known_buildings(),
                        load_jsonl(os.path.join(run_dir, "razdor-sync", "razdor.jsonl")),
                        load_jsonl(os.path.join(run_dir, "original", "original.jsonl")),
                        load_jsonl(os.path.join(run_dir, "original", "run.jsonl")),
                        load_jsonl(os.path.join(run_dir, "razdor-sync", "razdor-run.jsonl")))
    return known.classify(diff["local"], ctx), diff, ctx


def get_path(state, path):
    """The value at a differ path ('armies[id 3].x', 'battle.sides[1][0].hp', 'x.len')."""
    cur = state
    for tok in re.findall(r"[^.\[\]]+|\[[^\]]*\]", path):
        if cur is None:
            return None
        if tok.startswith("[id "):
            k = int(tok[4:-1])
            cur = next((x for x in cur if isinstance(x, dict) and x.get("id") == k), None)
        elif tok.startswith("["):
            i = int(tok[1:-1])
            cur = cur[i] if isinstance(cur, list) and i < len(cur) else None
        elif tok == "len" and isinstance(cur, list):
            cur = len(cur)
        else:
            cur = cur.get(tok) if isinstance(cur, dict) else None
    return cur


def signature(entries, ctx, step, map_stem=""):
    """The map, the action's op and the fields (ids left out): one candidate per signature.
    The map is part of it: the same fields on another map are often another cause."""
    paths = sorted({known.norm_path(e["path"]) for e in entries})
    op = ctx.actions[step].get("op") if step < len(ctx.actions) else "?"
    return f"{map_stem}:{op}:" + ",".join(paths)


def new_at(per_step, paths):
    """The first step where one of `paths` is classed NEW."""
    for s in sorted(per_step):
        if any(e["class"] == "new" and e["path"] in paths for e in per_step[s]):
            return s
    return None


# --- a NEW candidate ----------------------------------------------------------------------------
def shrink(actions, step, paths, info, razdor, name, budget):
    """Drop chunks of the actions before `step` while a NEW difference on one of `paths` still
    shows: returns (actions, step, run_dir of the last success or None, tries)."""
    best, best_step, best_dir, tries = actions[: step + 1], step, None, 0
    chunks = 2
    while tries < budget and len(best) > 3:
        middle = list(range(1, len(best) - 1))
        if not middle:
            break
        size = max(1, len(middle) // chunks)
        progressed = False
        for c in range(0, len(middle), size):
            if tries >= budget:
                break
            drop = set(middle[c:c + size])
            cand = [a for i, a in enumerate(best) if i not in drop]
            _, notes, ok = razdor.replay(cand)
            if not ok or notes.get(len(cand) - 1):
                continue   # the last action does not even apply in Razdor
            tries += 1
            d = run_both(cand, f"{name}-shrink{tries}")
            per, _, _ = classify_run(d, info)
            s = new_at(per, paths) if per else None
            print(f"  shrink try {tries}: {len(cand)} actions -> {'kept' if s is not None else 'lost'}", file=sys.stderr)
            if s is not None:
                best, best_step, best_dir = cand[: s + 1], s, d
                progressed = True
                break
        if not progressed:
            if size == 1:
                break
            chunks *= 2
    return best, best_step, best_dir, tries


def save_candidate(cid, info, hero, episode_actions, repro, step, entries, run_dir, rerun_dir,
                   diff_row, razdor, shrink_info, exe):
    out = os.path.join(EXPLORE, cid)
    os.makedirs(out, exist_ok=True)
    w = lambda name, acts: open(os.path.join(out, name), "w", encoding="utf-8").write(
        "".join(json.dumps(a, ensure_ascii=False) + "\n" for a in acts))
    w("episode.jsonl", episode_actions)
    w("repro.jsonl", repro)
    src = run_dir
    for side, rel in (("razdor", "razdor-sync/razdor.jsonl"), ("original", "original/original.jsonl")):
        st = {s["step"]: s for s in load_jsonl(os.path.join(src, rel))}
        if step in st:
            json.dump(st[step], open(os.path.join(out, f"state-{side}.json"), "w", encoding="utf-8"),
                      ensure_ascii=False, indent=1)
    shot = os.path.join(src, "original", f"shot-{step:04d}.png")
    if os.path.exists(shot):
        shutil.copy(shot, os.path.join(out, "shot-original.png"))
    razdor_shot(exe, os.path.join(out, "repro.jsonl"), step, os.path.join(out, "shot-razdor.png"), out)
    tr = os.path.join(rerun_dir or src, "original", "trace.jsonl")
    if os.path.exists(tr):
        with open(os.path.join(out, "trace-around.jsonl"), "w") as f:
            for l in open(tr):
                try:
                    if json.loads(l).get("s") in (step - 1, step):
                        f.write(l)
                except ValueError:
                    pass
    json.dump({"id": cid, "map": info.file, "hero": hero, "step": step, "fields": entries,
               "rng": diff_row.get("rng", []), "notes": diff_row.get("notes", []),
               "run": src, "rerun": rerun_dir, "shrink": shrink_info},
              open(os.path.join(out, "candidate.json"), "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    return out


def append_candidate_md(cid, info, hero, repro, step, entries, diff_row, shrink_info, out):
    out = out.replace(os.path.expanduser("~"), "~", 1)
    short = lambda v: (lambda s: s if len(s) <= 50 else s[:47] + "...")(json.dumps(v, ensure_ascii=False))
    a = repro[step]
    L = [f"## {cid}: {info.stem}, step {step} `{act_text(a)}`", "",
         f"- Found {datetime.date.today()} by explore.py (hero {hero}); unconfirmed.",
         f"- Repro: {len(repro)} actions (shrunk from {shrink_info['from']}, {shrink_info['tries']} "
         f"tries): `{out}/repro.jsonl`; `python -m tools.difftest.run --actions <it> --trace random`.",
         "- Step-local run, fields classed NEW (Razdor / original):"]
    for e in entries[:8]:
        L.append(f"  - `{e['path']}`: {short(e['razdor'])} / {short(e['original'])}")
    if len(entries) > 8:
        L.append(f"  - ... {len(entries) - 8} more")
    for s in diff_row.get("rng", [])[:5]:
        L.append(f"- rng: {s}")
    L.append(f"- The original gave the same values on a second run (trace `random,ai,events`). "
             f"Files: states, screenshots, `trace-around.jsonl` in `{out}/`.")
    L.append("")
    new_file = not os.path.exists(CANDIDATES) or os.path.getsize(CANDIDATES) == 0
    with open(CANDIDATES, "a", encoding="utf-8") as f:
        if new_file:
            f.write("# Diff test candidates\n\nDifferences the LLM explorer (`explore.py`) found that "
                    "`known.py` could not match to a FINDINGS.md entry or to noise. Each needs a "
                    "human (or Claude) to confirm it, explain it and move it to FINDINGS.md, or "
                    "teach `known.py` to recognise it.\n\n")
        f.write("\n".join(L) + "\n")


def load_signatures():
    p = os.path.join(EXPLORE, "signatures.json")
    return json.load(open(p)) if os.path.exists(p) else {}


def save_signatures(sigs):
    json.dump(sigs, open(os.path.join(EXPLORE, "signatures.json"), "w"), ensure_ascii=False, indent=1)


def investigate(name, actions, info, hero, per, diff, ctx, razdor, exe, shrink_budget, sigs, logf):
    """Handle the first NEW difference of an episode. Returns a result dict."""
    step, entries = known.first_of(per, "new")
    sig = signature(entries, ctx, step, info.stem)
    res = {"step": step, "signature": sig, "fields": [e["path"] for e in entries]}
    if sig in sigs:
        res["class"] = "seen"
        res["candidate"] = sigs[sig]
        return res
    # Noise: the original once more on the prefix, with more traces.
    prefix = actions[: step + 1]
    rerun = run_both(prefix, f"{name}-rerun", trace="random,ai,events")
    o1 = {s["step"]: s for s in load_jsonl(os.path.join(RUNS, name, "original", "original.jsonl"))}
    o2 = {s["step"]: s for s in load_jsonl(os.path.join(rerun, "original", "original.jsonl"))}
    if step not in o2:
        res["class"] = "harness"
        return res
    self_diff = [e["path"] for e in entries if get_path(o1[step], e["path"]) != get_path(o2[step], e["path"])]
    if self_diff:
        res["class"] = "noise"
        res["self_diff"] = self_diff
        return res
    per2, _, _ = classify_run(rerun, info)
    paths = {e["path"] for e in entries}
    if not per2 or new_at(per2, paths) != step:
        res["class"] = "unstable"
        return res
    repro, rstep, rdir, tries = shrink(prefix, step, paths, info, razdor, name, shrink_budget)
    rdir = rdir or rerun
    per3, diff3, _ = classify_run(rdir, info)
    entries3 = [e for e in per3.get(rstep, []) if e["class"] == "new"] or entries
    row = next((r for r in diff3["local"] if r["step"] == rstep), {})
    cid = "C" + datetime.datetime.now().strftime("%m%d-%H%M%S")
    out = save_candidate(cid, info, hero, actions, repro, rstep, entries3, rdir, rerun, row, razdor,
                         {"from": len(prefix), "to": len(repro), "tries": tries}, exe)
    append_candidate_md(cid, info, hero, repro, rstep, entries3, row,
                        {"from": len(prefix), "tries": tries}, out)
    sigs[sig] = cid
    save_signatures(sigs)
    res.update({"class": "NEW", "candidate": cid, "dir": out, "repro_len": len(repro), "repro_step": rstep})
    return res


# --- main -----------------------------------------------------------------------------------------
def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--hours", type=float, default=2.0, help="stop after this many hours")
    ap.add_argument("--episodes", type=int, default=0, help="stop after this many episodes (0: no limit)")
    ap.add_argument("--max-new", type=int, default=3, help="stop after this many NEW candidates")
    ap.add_argument("--len", default="40-80", help="episode length in actions, 'N' or 'MIN-MAX'")
    ap.add_argument("--maps", help="comma-separated map name prefixes (default: every startable map)")
    ap.add_argument("--install", default=DEFAULT_INSTALL)
    ap.add_argument("--ollama", default=os.environ.get("OLLAMA_HOST", "http://localhost:11434"))
    ap.add_argument("--model", default="qwen3.6:latest")
    ap.add_argument("--seed", type=int, default=None)
    ap.add_argument("--shrink-budget", type=int, default=4, help="original runs spent on shrinking a NEW one")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("--play-only", action="store_true", help="play episodes in Razdor only, no diff")
    ap.add_argument("--no-live", action="store_true",
                    help="do not play the original along (it is then played once after the episode)")
    a = ap.parse_args(argv)

    rnd = random.Random(a.seed)
    lo, _, hi = a.len.partition("-")
    lo, hi = int(lo), int(hi or lo)
    maps = startable_maps(a.install)
    if a.maps:
        want = [w.strip().lower() for w in a.maps.split(",")]
        maps = [m for m in maps if any(m.stem.lower().startswith(w) for w in want)]
    if not maps:
        raise SystemExit("no startable maps")
    os.makedirs(EXPLORE, exist_ok=True)
    target = os.path.join(CACHE, "target")
    exe = os.path.join(target, "release", "razdor") if a.no_build else build(target)
    razdor = Razdor(exe, os.path.join(EXPLORE, "tmp"))
    model = Model(a.ollama, a.model)
    if not model.available():
        raise SystemExit(f"Ollama at {a.ollama} does not answer for {a.model}; giving up")
    sigs = load_signatures()
    t_start = time.time()
    episode = 0
    found = 0
    skip = set()
    log_path = os.path.join(EXPLORE, "log.jsonl")
    goals = GOALS[:]
    rnd.shuffle(goals)
    while True:
        if a.episodes and episode >= a.episodes:
            break
        if time.time() - t_start > a.hours * 3600 or found >= a.max_new:
            break
        info = maps[episode % len(maps)]
        heroes = [h for h in info.heroes() if (info.stem, h) not in skip] or [1]
        # Each episode the next class (7 maps and 3 classes: every pair within 21 episodes).
        hero = heroes[episode % len(heroes)]
        length = rnd.randint(lo, hi)
        name = "ep" + datetime.datetime.now().strftime("%m%d-%H%M%S")
        goal_list = goals[episode % len(goals):] + goals[: episode % len(goals)]
        print(f"episode {episode} {name}: {info.stem}, hero {hero}, {length} actions, "
              f"first goal: {goal_list[0][:50]}", file=sys.stderr)
        t0 = time.time()
        calls0, secs0 = model.calls, model.seconds
        chat = open(os.path.join(EXPLORE, name + "-chat.jsonl"), "w", encoding="utf-8")
        logf = lambda rec: chat.write(json.dumps(rec, ensure_ascii=False) + "\n")
        live = None if a.play_only or a.no_live else LiveOriginal(os.path.join(RUNS, name + "-live"))
        try:
            acts, st, why = play_episode(info, hero, goal_list, razdor, model, length, rnd, logf, live)
        finally:
            if live:
                live.stop()
        chat.close()
        with open(os.path.join(EXPLORE, name + "-actions.jsonl"), "w", encoding="utf-8") as f:
            f.write("".join(json.dumps(x, ensure_ascii=False) + "\n" for x in acts))
        t_play = time.time() - t0
        rec = {"episode": episode, "name": name, "map": info.file, "hero": hero, "goal": goal_list[0],
               "length": len(acts), "target": length, "end": why, "play_s": round(t_play),
               "model_s": round(model.seconds - secs0), **st}
        print(f"  played {len(acts)} actions in {t_play:.0f}s ({why}); model calls {st['model_calls']}, "
              f"invalid: wrong screen {st['wrong_screen']}, rejected {st['rejected']}, too strong "
              f"{st['too_strong']}, dropped {st['dropped']}, repaired JSON {st['repaired_json']}; "
              f"rewinds {st['rewinds']}, resyncs {st['resyncs']}; ops {st['ops_razdor']} / original "
              f"{st['ops_original']}", file=sys.stderr)
        if live:
            rec["live_original"] = live.failed or "ok"
        if not a.play_only and len(acts) > 1:
            t1 = time.time()
            reuse = live.out if live and not live.failed and live.n == len(acts) else None
            d = run_both(acts, name, shots=True, reuse=reuse)
            per, diff, ctx = classify_run(d, info)
            if per is None:
                rec["result"] = {"class": "harness", "why": "no diff.json"}
            else:
                orig = load_jsonl(os.path.join(d, "original", "original.jsonl"))
                if len(orig) < 2:
                    rec["result"] = {"class": "harness", "why": "the original did not start"}
                    skip.add((info.stem, hero))
                else:
                    rec["original_steps"] = len(orig)
                    rec["free_first"] = diff.get("first")
                    rec["tally"] = known.tally(per)
                    hits = []
                    for s in sorted(per):
                        for e in per[s]:
                            if e["class"].startswith(("known", "noise")) and e["why"] != "unchanged since the step before":
                                hits.append([s, e["class"], e["path"]])
                    rec["hits"] = hits[:60]
                    ks, _ = known.first_of(per, "new")
                    if ks is None:
                        rec["result"] = {"class": "none"}
                    else:
                        res = investigate(name, acts, info, hero, per, diff, ctx, razdor, exe,
                                          a.shrink_budget, sigs, logf)
                        rec["result"] = res
                        if res.get("class") == "NEW":
                            found += 1
            rec["diff_s"] = round(time.time() - t1)
            print(f"  result: {rec.get('result')}", file=sys.stderr)
        rec["total_s"] = round(time.time() - t0)
        with open(log_path, "a", encoding="utf-8") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        episode += 1
    hours = (time.time() - t_start) / 3600
    print(f"done: {episode} episodes in {hours:.2f} h ({episode / hours if hours else 0:.1f}/h), "
          f"{found} NEW", file=sys.stderr)


if __name__ == "__main__":
    main()
