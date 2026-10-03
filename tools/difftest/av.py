"""Sounds, music and animations of both sides, step by step, and their differences.

The original's come from the Frida trace preset `av` (trace.py: Sound_Play 0x481420,
Music_Play 0x49d774, Music_NextRandom 0x49d7f8, and every entry pushed on the deferred-call
queues 0x48c2e4 / 0x48c348, which is how the game starts each timed animation); Razdor's from
the `av` list its replay writes per step into `razdor-run.jsonl` (src/av.rs, src/difftest.rs:
the same names at the points where the interface cues them).

An event is {"k": "sfx" | "music" | "anim", "n": name, "t": target or absent}:
- sfx: the `[SFX-Effects]` key of `_Sounds.ini` (`Battle-Fight`, `Global-Event-2`...);
- music: the `[Backgrounds]` key of the track started (`BkgBattle2`, `BkgTriumph`...);
- anim: `battle_slide`, `battle_effect:<melee|shot|magic|bless|cure>` (target `side:row:col`,
  side 1 the player's), `battle_pass`, `card_slide`, `battle_end_hold`, `walk`, `wait`,
  `camera_glide`, `reveal`, `look_at_army`, `world_spell`, `army_slot_slide`, `unit_action`,
  `promotion` (see QUEUE_FNS).

    python -m tools.difftest.av --run RUN_DIR            # the per-step differences of a run
    python -m tools.difftest.av --coverage RUN_DIR...    # the coverage table (AV.md)
"""

import argparse
import collections
import json
import os

from .trace import SOUND_GLOBALS

# The callbacks pushed on the deferred-call queues (interface.md §0 of the raw notes, world.md,
# magic-items.md, economy.md): what each animates.
QUEUE_FNS = {
    0x4AE280: "wait",              # WaitTimer_Tick: a wait, an event's delay, a spell's reading
    0x4AE6DC: "walk",              # the hero's walk along the route
    0x4AF2F8: "world_spell",       # a world spell's effect on an army
    0x4AF658: "chain",             # the next queued event step (sequencing, nothing shown)
    0x4AF83C: "reveal",            # the fog opening around a shown place
    0x4AF96C: "camera_glide",      # the camera's 900 ms glide
    0x4AFA98: "look_at_army",      # the camera to an army (a spell's target)
    0x4AFB54: "battle_pass",       # a pass: 100 ms with the busy pointer
    0x4AFBD8: "battle_slide",      # the action sprite from the actor's card to the target's
    0x4AFE7C: "battle_effect",     # the 25-frame effect on a card, its sound at the start
    0x4B0284: "card_slide",        # a card moving to another cell (battle, army exchange)
    0x4B09E8: "battle_end_hold",   # the won battle's hold: the experience cards (2500 ms)
    0x4B0C04: "army_slot_slide",   # a unit moved between armies / hired (building grids)
    0x4B11CC: "unit_action",       # heal, potion or dismiss of a unit in a building
    0x4B1A04: "promotion",         # the promotion screen
    0x4B2044: "portrait_highlight",  # the hero class portrait on the new-game screen
}
# Animations that only sequence other things: listed, not compared.
INTERNAL = {"chain", "portrait_highlight"}
# The effect pictures of 0x4afe7c (c & 0xff) and the sounds (b >> 16 & 0xff, table 0x4aff5b).
EFFECT_PICTURES = {0: "shot", 1: "melee", 2: "magic", 3: "bless", 4: "cure"}
EFFECT_SOUNDS = {0: "Battle-Shoot", 1: "Battle-Fight", 2: "Battle-Strike", 3: "Battle-Bless",
                 4: "Battle-Cure", 5: "Battle-Sorcery"}
# Card places (0x492940) → (row, col), the inverse of memread.WIDE_PLACES.
PLACES = {**{c - 1: (1, c) for c in range(1, 7)}, **{c + 5: (2, c) for c in range(2, 6)},
          6: (3, 3), 11: (3, 4)}

MUSIC = [n for _, n in SOUND_GLOBALS if n.startswith("Bkg")]
SFX = [n for _, n in SOUND_GLOBALS if not n.startswith("Bkg")]
ANIMS = sorted({n for n in QUEUE_FNS.values() if n not in ("battle_effect",) and n not in INTERNAL}
               | {f"battle_effect:{p}" for p in EFFECT_PICTURES.values()})


def load_jsonl(path):
    if not os.path.exists(path):
        return []
    return [json.loads(l) for l in open(path, encoding="utf-8") if l.strip()]


def card_name(side, place):
    """`side:row:col` of a battle card (the game's side byte: 1 the player's, 2 the enemy's,
    as the action list counts them; checked on РК1's ruins)."""
    rc = PLACES.get(place)
    return f"{side}:{rc[0]}:{rc[1]}" if rc else f"{side}:place{place}"


def queue_event(rec):
    fn = int(rec["a"]["fn"], 16)
    name = QUEUE_FNS.get(fn, f"fn_{fn:x}")
    entry = (rec.get("l") or {}).get("entry") or [fn, 0, 0, 0]
    _, a, b, c = entry
    t = None
    if name == "battle_effect":
        name = f"battle_effect:{EFFECT_PICTURES.get(c & 0xFF, c & 0xFF)}"
        t = card_name((b >> 8) & 0xFF, b & 0xFF)   # b = place | side << 8 | sound << 16
    elif name == "battle_slide":
        # b = from place | from side << 8 | to place << 16 | to side << 24: the actor's card.
        t = card_name((b >> 8) & 0xFF, b & 0xFF)
    elif name == "card_slide":
        t = card_name((b >> 8) & 0xFF, b & 0xFF)
    elif name == "camera_glide":
        t = f"{c & 0xFFFF},{c >> 16}"          # the cell packed x | y << 16
    elif name == "reveal":
        t = f"{b & 0xFFFF},{b >> 16}"
    elif name == "battle_end_hold":
        t = f"{b}ms"
    return {"k": "anim", "n": name, "t": t, "raw": [a, b, c]}


def original_events(records):
    """The original's events by step, from a trace.jsonl's records (preset `av`)."""
    out = collections.defaultdict(list)
    for r in records:
        f = r["f"]
        e = None
        if f == "SoundPlay":
            group = (r.get("e") or {}).get("group")
            name = (r.get("e") or {}).get("name") or f"slot{r['a']['slot']}"
            if group == 2:
                e = {"k": "sfx", "n": name}
        elif f == "MusicPlay":
            e = {"k": "music", "n": (r.get("e") or {}).get("name") or f"slot{r['a']['track']}"}
        elif f == "MusicNext":
            e = {"k": "music", "n": (r.get("l") or {}).get("name"), "t": "rotation"}
        elif f in ("QueuePush", "QueuePushFront"):
            e = queue_event(r)
        if e is not None:
            e["q"] = r["q"]
            out[r["s"]].append(e)
    for s in out:
        out[s].sort(key=lambda e: e["q"])
    return dict(out)


def razdor_events(run):
    """Razdor's events by step, from razdor-run.jsonl's lines."""
    return {e["step"]: list(e.get("av") or []) for e in run}


def _names(events, kind):
    return [e["n"] for e in events if e["k"] == kind and e["n"] not in INTERNAL]


def diff_step(o, r):
    """{kind: {"missing": [...], "extra": [...], "order": bool, "targets": [...]}} for the
    kinds that differ (missing: the original plays it, Razdor does not)."""
    out = {}
    for kind in ("sfx", "music", "anim"):
        a, b = _names(o, kind), _names(r, kind)
        ca, cb = collections.Counter(a), collections.Counter(b)
        missing = list((ca - cb).elements())
        extra = list((cb - ca).elements())
        common = ca & cb
        def keep(seq):
            left, res = collections.Counter(common), []
            for n in seq:
                if left[n] > 0:
                    left[n] -= 1
                    res.append(n)
            return res
        order = keep(a) != keep(b)
        targets = []
        if kind == "anim":
            ta = collections.defaultdict(list)
            for e in o:
                if e["k"] == "anim" and e.get("t") is not None:
                    ta[e["n"]].append(e["t"])
            tb = collections.defaultdict(list)
            for e in r:
                if e["k"] == "anim" and e.get("t") is not None:
                    tb[e["n"]].append(e["t"])
            for n in sorted(set(ta) & set(tb)):
                if n.startswith("battle_") and len(ta[n]) == len(tb[n]) and ta[n] != tb[n]:
                    targets.append((n, ta[n], tb[n]))
        if missing or extra or order or targets:
            out[kind] = {"missing": missing, "extra": extra, "order": order, "targets": targets}
    return out


def diff_run(steps, orig, raz):
    """Per step: the differences (diff_step) and both event lists."""
    rows = []
    for s in range(steps):
        o, r = orig.get(s, []), raz.get(s, [])
        rows.append({"step": s, "diff": diff_step(o, r), "original": o, "razdor": r})
    return rows


def _fmt(names):
    c = collections.Counter(names)
    return ", ".join(f"{n}×{k}" if k > 1 else n for n, k in c.items())


def report_lines(rows, actions, act_text):
    """The "Audio and effects" section of report.md."""
    L = ["## Audio and effects", ""]
    n_diff = sum(1 for r in rows if r["diff"])
    tot = collections.Counter()
    for r in rows:
        for kind, d in r["diff"].items():
            tot[(kind, "missing")] += len(d["missing"])
            tot[(kind, "extra")] += len(d["extra"])
            tot[(kind, "order")] += int(d["order"])
    L += [f"{len(rows) - n_diff} of {len(rows)} steps play the same sounds, tracks and animations "
          "(compared per kind: names as a multiset, then the order of the common ones; "
          "battle targets as `side:row:col`).", ""]
    L += ["| kind | missing in Razdor | extra in Razdor | steps with another order |", "|---|---|---|---|"]
    for kind in ("sfx", "music", "anim"):
        L += [f"| {kind} | {tot[(kind, 'missing')]} | {tot[(kind, 'extra')]} | {tot[(kind, 'order')]} |"]
    L += ["", "| step | action | kind | missing in Razdor | extra in Razdor | order / targets |",
          "|---|---|---|---|---|---|"]
    for r in rows:
        for kind, d in r["diff"].items():
            note = "order differs" if d["order"] else ""
            for n, a, b in d["targets"][:2]:
                note += ("; " if note else "") + f"{n}: {a} vs {b}"
            act = act_text(actions[r["step"]]) if r["step"] < len(actions) else "-"
            L += [f"| {r['step']} | `{act}` | {kind} | {_fmt(d['missing'])} | {_fmt(d['extra'])} | {note} |"]
    L += [""]
    return L


# --- coverage ----------------------------------------------------------------------------------
def coverage(run_dirs):
    """Counts per (kind, name) over runs: seen in the original, played by Razdor."""
    seen_o, seen_r = collections.Counter(), collections.Counter()
    for d in run_dirs:
        orig = original_events(load_jsonl(os.path.join(d, "original", "trace.jsonl")))
        # The step-local run when there is one (the chords and music picks are draws).
        side = "razdor-sync" if os.path.exists(os.path.join(d, "razdor-sync", "razdor-run.jsonl")) else "razdor"
        raz = razdor_events(load_jsonl(os.path.join(d, side, "razdor-run.jsonl")))
        for evs in orig.values():
            for e in evs:
                seen_o[(e["k"], e["n"])] += 1
        for evs in raz.values():
            for e in evs:
                seen_r[(e["k"], e["n"])] += 1
    return seen_o, seen_r


def coverage_rows(seen_o, seen_r):
    rows = []
    names = [("sfx", n) for n in SFX] + [("music", n) for n in MUSIC] + [("anim", n) for n in ANIMS]
    names += sorted((k for k in set(seen_o) | set(seen_r) if k not in names and k[1] not in INTERNAL))
    for k, n in names:
        o, r = seen_o.get((k, n), 0), seen_r.get((k, n), 0)
        status = ("never triggered yet" if not o and not r else "missing in Razdor" if o and not r
                  else "only Razdor" if r and not o else "both")
        rows.append((k, n, o, r, status))
    return rows


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--run", help="a run folder: print the per-step differences")
    ap.add_argument("--coverage", nargs="+", metavar="RUN_DIR", help="print the coverage table")
    ap.add_argument("--events", action="store_true", help="with --run: print both sides' events")
    a = ap.parse_args(argv)
    if a.run:
        acts = load_jsonl(os.path.join(a.run, "actions.jsonl"))
        orig = original_events(load_jsonl(os.path.join(a.run, "original", "trace.jsonl")))
        raz = razdor_events(load_jsonl(os.path.join(a.run, "razdor", "razdor-run.jsonl")))
        rows = diff_run(len(acts), orig, raz)
        for r in rows:
            if a.events:
                fmt = lambda es: " ".join(f"{e['k'][0]}:{e['n']}" + (f"@{e['t']}" if e.get("t") else "")
                                          for e in es if e["n"] not in INTERNAL)
                print(f"{r['step']:>3} {json.dumps(acts[r['step']], ensure_ascii=False)[:70]}")
                print(f"     O: {fmt(r['original'])}")
                print(f"     R: {fmt(r['razdor'])}")
            elif r["diff"]:
                print(r["step"], json.dumps(r["diff"], ensure_ascii=False))
    if a.coverage:
        so, sr = coverage(a.coverage)
        print("| kind | name | original | Razdor | status |")
        print("|---|---|---|---|---|")
        for k, n, o, r, st in coverage_rows(so, sr):
            print(f"| {k} | `{n}` | {o} | {r} | {st} |")


if __name__ == "__main__":
    main()
