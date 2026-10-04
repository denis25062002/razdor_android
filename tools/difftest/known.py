"""Sort the differences of a diff-test run into known findings, noise and new ones.

A small matcher over `run.py`'s output: each field that differs at a step of the step-local
run (each Razdor step starts from the original's generator state) is given a class:

- `known:N`      a difference FINDINGS.md entry N explains (rules below, by field and context);
- `downstream:N` a field a known finding has already thrown off earlier in the run (taint);
- `noise`        the original's frame noise (FINDINGS.md §5, first part): an AI army one cell
                 off while the generator agrees;
- `timing`       a result of the event the original shows that it applies only at OK
                 (FINDINGS.md "Not differences"), when the run ends before the OK; also the
                 events Razdor counts as done that the original fires only when the window
                 on screen is closed (queued behind it; candidate C1003-174531); the
                 tribute while the original's village window is open (paid as it closes);
                 and a market's goods after a purchase while the original's building
                 window is open (the slot is written back as it closes; C1004-050909);
- `harness`      one side has no state at that step (the original's harness stopped);
- `new`          none of the above: a candidate for a human to look at.

The rules, by FINDINGS.md entry:
1. `rng`, the original ahead, its extra draws all Random(3000) "patroller idle offset" (the
   draw for each idle patroller when the hero stops) and the other draws the same `n` in the
   same order. Taints nothing (the step-local run starts the next step in sync).
2. a village entered while an event fires: `rng` with Razdor's village-offer draws
   (rules/economy.rs) at a step where the original shows the event, and the hero's gold/mana
   and that village's fields. Taints the village's fields from then on.
3. `battle.sides[0]...row/col/type` (the hero's starting formation). Taints everything after
   it: the battle runs otherwise from there.
4. `battle.sides[1]...hp` in a battle with a ruins' garrison. Taints everything.
5. first part: an AI army one cell off while the generator agrees (or differs only by §1's
   draws) is the original's frame noise (class `noise`).
5. second part, `rng` where both sides drew the same `n` (Random(3000) aside) but in another
   order and the draws include the AI's wander points: arrivals within a tick come in another
   order. Only when the AI's draws at the first difference are each drawn by the other side
   too in that step: a wander call's pair of ranges (its box) and an AI hire's XP range. A
   range only one side draws (a wander box or area of its own) is `new`. Also a midnight's
   draws (barracks, market restock) against the AI's or another building's midnight draws:
   the midnight runs at the end of the original's frame, after that frame's arrivals, and at
   the frame's minute, which decides whether a market whose timer falls a minute after the
   midnight restocks (FINDINGS §5, third part). Taints the armies,
   the buildings' goods, gold and owners, and later generator differences whose draws involve
   the AI.
6. `hero.units[k].xp` (or `.level`) higher in Razdor: it pays battle XP at the gameplay
   video's rate, `HeroExpirienceModificator` 100 where the install has 50 (deliberate, the
   user's choice of 2026-09-29). Taints the hero's units (levels and their HP follow).

Desync: when one side skips an action (it needs another screen) that the other applies, the
sides are on different screens and everything after differs: `screen` is classed downstream
of a known finding hit in the 3 steps before (e.g. §2's village offer that is a yes/no
question in the original only), else NEW; everything after is tainted.

A field equal to the same pair of values one step earlier keeps that step's class (a
difference that simply stays).
"""

import re
from collections import Counter

PATROL_IDLE = 3000
AI_SITES = ("AI wander points", "AI hire XP", "ai.rs")
WANDER_DRAWS = 8   # four points, x then y (0x4a2550)
MIDNIGHT_SITES = ("barracks", "market restock", "economy.rs")


def _ai(site):
    return any(s in site for s in AI_SITES)


def wander_calls(seq):
    """{start index: (x range, y range)} of the wander calls in a step's draws `seq` [(n,
    site)]: eight AI draws alternating two ranges (Razdor's sites are only file:line)."""
    out, j = {}, 0
    while j + WANDER_DRAWS <= len(seq):
        run = seq[j:j + WANDER_DRAWS]
        ns = [n for n, _ in run]
        if all(_ai(site) for _, site in run) and ns[0::2] == [ns[0]] * 4 and ns[1::2] == [ns[1]] * 4:
            for k in range(WANDER_DRAWS):
                out[j + k] = (ns[0], ns[1])
            j += WANDER_DRAWS
        else:
            j += 1
    return out


def same_ai_draws(o, r, i):
    """Whether the AI draws at the first difference `i` of `o` and `r` are each drawn by the
    other side too in the step: the same wander box (pair of ranges), the same hire range."""
    wo, wr = wander_calls(o), wander_calls(r)
    for mine, theirs, wmine, wtheirs in ((o, r, wo, wr), (r, o, wr, wo)):
        if i >= len(mine):
            continue
        if i in wmine:
            if wmine[i] not in wtheirs.values():
                return False
        elif mine[i][0] not in [n for n, _ in theirs]:
            return False
    return True


def norm_path(p):
    """'armies[id 3].x' -> 'armies[].x' (for signatures)."""
    return re.sub(r"\[(id )?\d+\]", "[]", p)


def _army_id(p):
    m = re.match(r"armies\[id (\d+)\]\.(x|y)$", p)
    return int(m.group(1)) if m else None


def _building_id(p):
    m = re.match(r"buildings\[id (\d+)\]", p)
    return int(m.group(1)) if m else None


class Context:
    """What the matcher needs to know about a run besides the diffs."""

    def __init__(self, actions, buildings, raz_states, orig_states, orig_run, raz_run):
        self.actions = actions
        self.buildings = buildings          # {id: (type, x, y, size_x, size_y)}
        self.r = {s["step"]: s for s in raz_states}
        self.o = {s["step"]: s for s in orig_states}
        self.orun = {e["step"]: e for e in orig_run}
        self.rrun = {e["step"]: e for e in raz_run}

    def o_draws(self, step):
        return [(d[0], d[3] if len(d) > 3 else "") for d in self.orun.get(step, {}).get("draws", [])]

    def r_draws(self, step):
        return [(d[0], d[2]) for d in self.rrun.get(step, {}).get("draws", [])]

    def desync(self, step):
        """'razdor applied / original: <note>' when one side skipped the step's action and
        the other did not, else None. Only skips of an action that needs another screen."""
        o_note = self.orun.get(step, {}).get("note") or ""
        r_notes = self.rrun.get(step, {}).get("notes") or []
        o_skip = o_note.startswith("skipped")
        r_skip = any(("takes no input" in n or "no battle" in n or "no question" in n
                      or "a question is shown" in n or "not the player's turn" in n
                      or "no building window" in n) for n in r_notes)
        if o_skip and not r_notes:
            return f"razdor applied / original {o_note}"
        if r_skip and not o_note:
            return f"razdor {r_notes[0].split(': ', 1)[-1]} / original applied"
        return None

    def event_window(self, step):
        """The map event (1-based) the original shows at `step` when Razdor has fired it."""
        meta = self.orun.get(step, {}).get("meta") or {}
        ev = meta.get("dialog_event", -1)
        if meta.get("screen") != "event" or ev is None or not 0 <= ev < meta.get("event_count", 1 << 30):
            return None
        return ev + 1 if ev + 1 in self.r.get(step, {}).get("events_done", []) else None

    def shown_event(self, step):
        """The map event (1-based) the original's event window shows at `step`, else None."""
        meta = self.orun.get(step, {}).get("meta") or {}
        ev = meta.get("dialog_event", -1)
        if meta.get("screen") != "event" or ev is None or not 0 <= ev < meta.get("event_count", 1 << 30):
            return None
        return ev + 1

    def screen(self, step):
        return (self.orun.get(step, {}).get("meta") or {}).get("screen")

    def bought_in_window(self, step):
        """Whether a `buy` was played since the original's building window opened (it stays
        open through `step`)."""
        i = step
        while i > 0 and self.screen(i) == "building":
            if (self.actions[i] if i < len(self.actions) else {}).get("op") == "buy":
                return True
            i -= 1
        return False

    def building_at(self, x, y):
        for bid, (t, bx, by, sx, sy) in self.buildings.items():
            if bx - max(sx, 1) < x <= bx and by - max(sy, 1) < y <= by:
                return bid, t
        return None, None

    def battle_foe_type(self, step):
        """The building type of the garrison fought at `step` (the last click before it)."""
        for i in range(step, -1, -1):
            a = self.actions[i] if i < len(self.actions) else {}
            if a.get("op") == "click_map":
                return self.building_at(a["x"], a["y"])[1]
        return None


def rng_rule(ctx, step):
    """The FINDINGS entry explaining a generator difference at `step`, or None."""
    o, r = ctx.o_draws(step), ctx.r_draws(step)
    if not ctx.orun.get(step, {}).get("draws") and o == []:
        return None  # no trace: cannot tell
    o_rest = [n for n, site in o if not (n == PATROL_IDLE and "patroller" in site)]
    r_n = [n for n, _ in r]
    extra = len(o) - len(o_rest)
    if o_rest == r_n and extra:
        return 1
    village = any("economy.rs" in site for _, site in r) and \
        not any("village" in site.lower() for _, site in o) and ctx.screen(step) == "event"
    if village:
        return 2
    # The first draw where the two sides' `n` part (the stop's Random(3000) aside) is one of
    # the AI's (wander points): the armies' draws came in another order (§5), and other
    # wander points then lead to other draws.
    o_rest_sites = [(n, site) for n, site in o if not (n == PATROL_IDLE and "patroller" in site)]
    i = 0
    while i < min(len(o_rest_sites), len(r)) and o_rest_sites[i][0] == r[i][0]:
        i += 1
    at = [site for seq in (o_rest_sites, r) if i < len(seq) for site in [seq[i][1]]]
    if at and all(_ai(site) for site in at) and same_ai_draws(o_rest_sites, r, i):
        return 5
    midnight = lambda site: any(s in site for s in MIDNIGHT_SITES)
    if len(at) == 2 and any(midnight(site) for site in at) and all(midnight(site) or _ai(site) for site in at):
        return 5
    return None


def classify(rows, ctx):
    """Per step of `rows` (diff.json's "local" or "free"): a list of
    {"path", "razdor", "original", "class", "why"}. Returns (per_step, hits) where hits
    lists (step, class, path) of each first known/new/noise classification."""
    taint = []          # (regex, entry)
    taint_all = None    # entry that taints everything
    prev = {}           # path -> (a, b, class, why) at the step before
    noise_armies = set()
    last_known = None   # (step, entry) of the latest known finding
    out = {}
    for row in rows:
        step = row["step"]
        cur = {}
        res = []
        desync = None if taint_all else ctx.desync(step)
        if desync:
            # The sides are on different screens: one applied the action, the other skipped
            # it. Everything from here differs for that reason.
            near = last_known if last_known and step - last_known[0] <= 3 else None
            cls = f"downstream:{near[1]}" if near else "new"
            why = (f"the sides are on different screens ({desync}), after FINDINGS §{near[1]} "
                   f"at step {near[0]}") if near else f"the sides are on different screens ({desync})"
            res.append({"path": "screen", "razdor": desync.split(" / ")[0],
                        "original": desync.split(" / ")[-1], "class": cls, "why": why})
            taint_all = near[1] if near else "desync"
        # The generator first, then the battle formation (its rule taints all the rest).
        order = lambda d: (d[0] != "rng", not re.match(r"battle\.sides\[0\].*\.(row|col|type|len)$", d[0]), d[0])
        diffs = sorted(row.get("diffs", []), key=order)
        rng_diff = any(p == "rng" for p, _, _ in diffs)   # cleared below when only §1's draws
        for p, a, b in diffs:
            cls, why = None, ""
            if p == "state":
                cls, why = "harness", "one side has no state at this step"
            elif taint_all:
                cls, why = f"downstream:{taint_all}", "after a known difference that changes the rest of the run"
            elif p in prev and prev[p][:2] == (a, b):
                cls, why = prev[p][2], "unchanged since the step before"
            else:
                for rx, entry in taint:
                    if re.match(rx, p):
                        cls, why = f"downstream:{entry}", f"a field FINDINGS §{entry} threw off earlier"
                        break
            if cls is None and ctx.event_window(step) and re.match(
                    r"hero\.(gold|mana|units)|buildings\[|armies\[id \d+\]\.(active|alive)", p):
                cls, why = "timing", (f"event {ctx.event_window(step)} is on screen in the original, which "
                                      "applies its results at OK (FINDINGS 'Not differences')")
            if cls is None and ctx.screen(step) == "village" and re.match(r"hero\.(gold|mana)$|buildings\[id \d+\]\.(gold|mana)$", p):
                cls, why = "timing", ("the original's village window is open: it pays the tribute when the "
                                      "window closes, Razdor on entering (FINDINGS 'Not differences')")
            if cls is None and re.match(r"buildings\[id \d+\]\.goods$", p) and ctx.screen(step) == "building" and \
                    ctx.bought_in_window(step) and isinstance(a, list) and isinstance(b, list) and \
                    not Counter(a) - Counter(b):
                cls, why = "timing", ("the original's building window is open after a purchase: it writes the "
                                      "emptied market slot back to the building when the window closes "
                                      "(economy.md, player market), Razdor at once")
            if cls is None and p == "events_done" and isinstance(a, list) and isinstance(b, list) and \
                    set(b) - set(a) and set(b) - set(a) == {ctx.shown_event(step)}:
                cls, why = "timing", (f"event {ctx.shown_event(step)} is on screen in the original, which the "
                                      "differ counts as done; Razdor counts a question only once it is answered")
            if cls is None and p == "events_done" and ctx.event_window(step) and \
                    isinstance(a, list) and isinstance(b, list) and set(b) <= set(a):
                cls, why = "timing", (f"event {ctx.event_window(step)} is on screen in the original; the "
                                      "events after it fire when it is closed (FINDINGS 'Not differences')")
            if cls is None and p == "rng":
                e = rng_rule(ctx, step)
                if e == 5 and taint and any(en == 5 for _, en in taint):
                    cls, why = "downstream:5", "the AI's draws after an arrival-order difference"
                elif e:
                    if e == 1:
                        rng_diff = False   # only the stop's Random(3000): no effect on the armies
                    cls, why = f"known:{e}", {1: "extra Random(3000) per idle patroller at the stop",
                                              2: "village offer rolled at arrival, not after the event",
                                              5: "the AI's or the midnight's draws in another order (arrivals, the midnight's place in the frame)"}[e]
                    if e == 5:
                        taint += [(r"armies\[", 5), (r"buildings\[id \d+\]\.(goods|gold|owner|mana)", 5)]
                elif any(e2 == 5 for _, e2 in taint) and \
                        any(_ai(site) for _, site in ctx.o_draws(step) + ctx.r_draws(step)):
                    cls, why = "downstream:5", "the AI's draws after an arrival-order difference"
            if cls is None and p.startswith("battle.sides[0]") and re.search(r"\.(row|col|type)$|\.len$", p):
                cls, why = "known:3", "the hero's starting formation (auto-arranged at load in the original)"
                taint_all = 3
            if cls is None and re.match(r"battle\.sides\[1\]\[\d+\]\.hp$", p) and ctx.battle_foe_type(step) == 12:
                cls, why = "known:4", "the ruins' garrison wears the ruins' goods in the original"
                taint_all = 4
            if cls is None and (p in ("hero.gold", "hero.mana", "rng") or _building_id(p)):
                bid = _building_id(p)
                for i in (step, step - 1):
                    a_ = ctx.actions[i] if 0 <= i < len(ctx.actions) else {}
                    if a_.get("op") != "click_map":
                        continue
                    vb, vt = ctx.building_at(a_["x"], a_["y"])
                    if vt == 2 and (bid in (None, vb)) and any(ctx.screen(j) == "event" for j in (i, i + 1)):
                        cls, why = "known:2", f"village {vb} entered while an event fires"
                        taint.append((rf"buildings\[id {vb}\]", 2))
                        break
            if cls is None and _army_id(p) is not None and not rng_diff:
                k = _army_id(p)
                rs = next((x for x in ctx.r.get(step, {}).get("armies", []) if x.get("id") == k), None)
                os_ = next((x for x in ctx.o.get(step, {}).get("armies", []) if x.get("id") == k), None)
                if rs and os_ and "x" in rs and "x" in os_ and \
                        max(abs(rs["x"] - os_["x"]), abs(rs["y"] - os_["y"])) <= 1:
                    cls, why = "noise", "an AI army one cell off with the generator in step (FINDINGS §5 frame noise)"
                    noise_armies.add(k)
            if cls is None and _army_id(p) in noise_armies and not rng_diff:
                cls, why = "noise", "the army that was one cell off (frame noise) catching up"
            if cls is None:
                cls, why = "new", ""
            res.append({"path": p, "razdor": a, "original": b, "class": cls, "why": why})
            cur[p] = (a, b, cls, why)
            if cls.startswith("known:"):
                last_known = (step, cls.split(":")[1])
        prev = cur
        out[step] = res
    return out


def first_of(per_step, cls_prefix):
    """(step, [entries]) of the first step with a class starting with `cls_prefix`."""
    for step in sorted(per_step):
        hit = [e for e in per_step[step] if e["class"].startswith(cls_prefix)]
        if hit:
            return step, hit
    return None, []


def tally(per_step):
    """Class -> number of fields over all steps (first appearance only: an unchanged field is
    not counted again)."""
    out = {}
    for step in sorted(per_step):
        for e in per_step[step]:
            if e["why"] == "unchanged since the step before":
                continue
            out[e["class"]] = out.get(e["class"], 0) + 1
    return out
