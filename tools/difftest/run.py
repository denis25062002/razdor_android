"""Play one action list in Razdor and in the original Discord Times and diff the two games.

    ~/.local/opt/re-venv/bin/python -m tools.difftest.run --actions rk1.jsonl --map РК1 --hero 1

Steps:
1. builds Razdor (release, without sound, into `~/.cache/razdor-difftest/target`);
2. `razdor --replay` plays the list (the free run);
3. `python -m tools.difftest.original` plays it in the original under Wine on a hidden Xvfb
   display, with the draw trace on and the timed music change held off (see README.md);
4. `razdor --replay --rng-from original.jsonl` plays it again with each step starting from the
   original's generator state of the step before (the step-local run: a divergence of the
   generator in one step does not spill into the next ones);
5. compares the states step by step and writes `report.md` (and `diff.json`) into the run
   folder `~/.cache/razdor-difftest/runs/<name>/`, with screenshots of both sides at the first
   divergence (Razdor's through `RAZDOR_SCENE=replay`, on its own hidden display).

The comparison takes the fields both sides have (the others are listed as missing) and
normalises one known difference of timing: the original counts an event as done when its
window closes, Razdor when it fires, so the event the original shows is counted as done.
A generator mismatch is explained from the two draw traces (each draw's `n` and site) and by
stepping the generator: "the original made k more draws".
"""

import argparse
import datetime
import json
import os
import shutil
import subprocess
import sys
import time

from .original import free_display, read_actions

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CACHE = os.path.expanduser("~/.cache/razdor-difftest")
PY = sys.executable


# --- the generator -----------------------------------------------------------------------------
def lcg(s):
    return (s * 214013 + 2531011) & 0xFFFFFFFF


def lcg_offset(a, b, limit=20000):
    """k such that b is a stepped k times (k > 0), or a is b stepped -k times; None if
    neither within `limit`."""
    if a == b:
        return 0
    x, y = a, b
    for k in range(1, limit + 1):
        x, y = lcg(x), lcg(y)
        if x == b:
            return k
        if y == a:
            return -k
    return None


# --- running the sides -------------------------------------------------------------------------
def build(target):
    cmd = ["cargo", "build", "--release", "--no-default-features", "--target-dir", target]
    print("build:", " ".join(cmd), file=sys.stderr)
    subprocess.run(cmd, cwd=REPO, check=True)
    return os.path.join(target, "release", "razdor")


def run_razdor(exe, actions, out, rng_from=None):
    os.makedirs(out, exist_ok=True)
    cmd = [exe, "--replay", actions, "--out", out]
    if rng_from:
        cmd += ["--rng-from", rng_from]
    res = subprocess.run(cmd, cwd=REPO, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    with open(os.path.join(out, "stderr.txt"), "w") as f:
        f.write(res.stderr)
    if res.returncode != 0:
        raise SystemExit(f"razdor --replay failed ({res.returncode}): {res.stderr[-2000:]}")


def run_original(actions, out, real_music=False, trace=True, frida=None):
    cmd = [PY, "-m", "tools.difftest.original", "--actions", actions, "--out", out]
    if trace:
        cmd.append("--trace-draws")
    if frida:
        cmd += ["--trace", frida]
    if not real_music:
        cmd.append("--hold-music")
    print("original:", " ".join(cmd), file=sys.stderr)
    res = subprocess.run(cmd, cwd=REPO)
    if res.returncode not in (0,):
        print(f"warning: the original's run ended with {res.returncode} (see {out})", file=sys.stderr)
    return res.returncode


def razdor_shot(exe, actions, step, path, work):
    """A picture of Razdor's screen after `step` (RAZDOR_SCENE=replay) on a hidden display."""
    n = free_display(90)
    disp = f":{n}"
    xvfb = subprocess.Popen(["Xvfb", disp, "-screen", "0", "1024x768x24", "-nolisten", "tcp"],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        for _ in range(100):
            if os.path.exists(f"/tmp/.X11-unix/X{n}"):
                break
            time.sleep(0.05)
        rt = os.path.join(work, "xdg-runtime")
        os.makedirs(rt, mode=0o700, exist_ok=True)
        env = dict(os.environ)
        env.pop("WAYLAND_DISPLAY", None)
        env.update(DISPLAY=disp, RAZDOR_SCENE=f"replay:{step}", RAZDOR_REPLAY=actions,
                   RAZDOR_SNAPSHOT=path, RAZDOR_SNAPSHOT_FRAMES="30", RAZDOR_SIZE="1024x768",
                   XDG_RUNTIME_DIR=rt, RAZDOR_LOG=os.path.join(work, "razdor-shot.log"),
                   RAZDOR_SAVE_DIR=os.path.join(work, "razdor-saves"))
        subprocess.run([exe], cwd=REPO, env=env, timeout=120, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        pass
    finally:
        xvfb.terminate()
        try:
            xvfb.wait(5)
        except subprocess.TimeoutExpired:
            xvfb.kill()
    return os.path.exists(path)


# --- comparing ---------------------------------------------------------------------------------
def load_jsonl(path):
    if not os.path.exists(path):
        return []
    return [json.loads(l) for l in open(path, encoding="utf-8") if l.strip()]


def compare(a, b, path=""):
    """(diffs, missing): diffs = [(path, a, b)] of fields both sides have; missing =
    [(path, side)] of fields only one side has ('razdor' / 'original' = who has it)."""
    diffs, missing = [], []
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b), key=str):
            p = f"{path}.{k}" if path else str(k)
            if k not in b:
                missing.append((p, "razdor"))
            elif k not in a:
                missing.append((p, "original"))
            else:
                d, m = compare(a[k], b[k], p)
                diffs += d
                missing += m
    elif isinstance(a, list) and isinstance(b, list) and a + b and \
            all(isinstance(x, (dict, list)) for x in a + b):
        if len(a) != len(b):
            diffs.append((f"{path}.len", len(a), len(b)))
        for i, (x, y) in enumerate(zip(a, b)):
            ids = isinstance(x, dict) and isinstance(y, dict) and "id" in x and x.get("id") == y.get("id")
            key = f"[id {x['id']}]" if ids else f"[{i}]"
            d, m = compare(x, y, path + key)
            diffs += d
            missing += m
    elif a != b:
        diffs.append((path, a, b))
    return diffs, missing


def normalise_original(st, meta):
    """The original's state at Razdor's point of view: the event shown counts as done."""
    st = json.loads(json.dumps(st))
    ev = meta.get("dialog_event", -1) if meta else -1
    # Only the map's events: the engine's own reports (the battle result...) use a slot past
    # the last event.
    real = ev is not None and 0 <= ev < (meta or {}).get("event_count", 1 << 30)
    if meta and meta.get("screen") == "event" and real:
        done = set(st.get("events_done", []))
        if ev + 1 not in done:
            st["events_done"] = sorted(done | {ev + 1})
            st["_normalised"] = f"event {ev + 1} shown, counted as done"
    return st


def draw_list_razdor(entry):
    return [(d[0], d[1], d[2].split("src/")[-1]) for d in (entry or {}).get("draws", [])]


def draw_list_original(entry):
    return [(d[0], d[1], f"{d[3]} {d[2]}") for d in (entry or {}).get("draws", [])]


def explain_rng(step, r_state, o_state, r_draws, o_draws, o_meta_prev, o_meta, traced, lost,
                music_held=True):
    """Why the generators differ after this step (a list of sentences)."""
    out = []
    k = lcg_offset(r_state, o_state)
    if k is None:
        out.append("the states are not on the same stream within 20000 draws")
    elif k > 0:
        out.append(f"the original is {k} draw(s) ahead of Razdor")
    elif k < 0:
        out.append(f"Razdor is {-k} draw(s) ahead of the original")
    if traced and not lost:
        rn = [d[0] for d in r_draws]
        on = [d[0] for d in o_draws]
        i = 0
        while i < min(len(rn), len(on)) and rn[i] == on[i]:
            i += 1
        same_start = r_draws[0][1] == o_draws[0][1] if r_draws and o_draws else None
        out.append(f"draws this step: Razdor {len(rn)}, original {len(on)}; the n agree for the "
                   f"first {i}" + ("" if same_start in (None, True) else
                                    " (but the step started from different states)"))
        if i < len(on):
            out.append("original from there: " + ", ".join(
                f"Random({n}) {site}" for n, _, site in o_draws[i:i + 8]) +
                (" ..." if len(on) - i > 8 else ""))
        if i < len(rn):
            out.append("Razdor from there: " + ", ".join(
                f"Random({n}) {site}" for n, _, site in r_draws[i:i + 8]) +
                (" ..." if len(rn) - i > 8 else ""))
        music = [d for d in o_draws if "music" in d[2]]
        if music and music_held:
            out.append(f"{len(music)} of the original's draws are a music change that is not the "
                       "timer (held off in this run): a track started by the game (the triumph's "
                       "end, a battle)")
        elif music:
            out.append(f"REAL-TIME NOISE: {len(music)} of the original's draws are a music change, "
                       "possibly the timed one (engine.md §3.4), which a replay cannot place")
    elif traced and lost:
        out.append(f"the original's trace lost {lost} draws this step (ring full)")
    if not traced and o_meta_prev and o_meta:
        due = o_meta_prev.get("next_music_ms")
        if due is not None and o_meta.get("now_ms", 0) >= due:
            out.append("REAL-TIME NOISE: the original's music timer ran out during this step "
                       "(a Random(8) and a Random(50000-90000) draw)")
    return out


def diff_runs(actions, raz, orig, orig_run, raz_run, music_held=True):
    rows = []
    o_by_step = {s["step"]: s for s in orig}
    r_by_step = {s["step"]: s for s in raz}
    run_by_step = {e["step"]: e for e in orig_run}
    rrun_by_step = {e["step"]: e for e in raz_run}
    first = None
    for step, act in enumerate(actions):
        r, o = r_by_step.get(step), o_by_step.get(step)
        e = run_by_step.get(step, {})
        row = {"step": step, "action": act, "notes": [], "diffs": [], "missing": [], "rng": []}
        if e.get("note"):
            row["notes"].append("original: " + e["note"])
        row["notes"] += ["razdor: " + n.split(": ", 1)[-1] for n in rrun_by_step.get(step, {}).get("notes", [])]
        if r is None or o is None:
            row["diffs"].append(("state", "razdor" if r else "-", "original" if o else "-"))
            rows.append(row)
            continue
        on = normalise_original(o, e.get("meta"))
        if "_normalised" in on:
            row["notes"].append("normalised: " + on.pop("_normalised"))
        d, m = compare(r, on)
        d = [x for x in d if x[0] not in ("step", "map")]
        row["diffs"], row["missing"] = d, m
        if r["rng"] != o["rng"]:
            prev = run_by_step.get(step - 1, {}).get("meta")
            row["rng"] = explain_rng(step, r["rng"], o["rng"], draw_list_razdor(rrun_by_step.get(step)),
                                     draw_list_original(e), prev, e.get("meta"), "draws" in e,
                                     e.get("draws_lost", 0), music_held)
        rows.append(row)
    mark_window_timing(rows, run_by_step)
    for row in rows:
        if row["diffs"] and first is None:
            first = row["step"]
    return rows, first


WINDOWS = ("event", "village", "building", "shipyard", "question")
CLOSING = ("ok", "answer", "key")


def mark_window_timing(rows, run_by_step):
    """A difference seen while the original shows a window (an event's, a village's...) that
    is gone once the window is closed (only closing actions in between) is a matter of when
    each side applies a result (the original: an event's finish results, the village's gold
    when its window closes): it moves from `diffs` to `timing`."""
    screen = lambda i: run_by_step.get(i, {}).get("meta", {}).get("screen")
    for i, row in enumerate(rows):
        row.setdefault("timing", [])
        if screen(i) not in WINDOWS or not row["diffs"]:
            continue
        j = i + 1
        while j < len(rows) and screen(j) in WINDOWS and rows[j]["action"].get("op") in CLOSING:
            j += 1
        if j >= len(rows) or rows[j]["action"].get("op") not in CLOSING or screen(j) in WINDOWS:
            continue
        later = {p for p, _, _ in rows[j]["diffs"]}
        keep = []
        for d in row["diffs"]:
            (keep if d[0] in later or d[0] == "rng" else row["timing"]).append(d)
        row["diffs"] = keep


# --- the report --------------------------------------------------------------------------------
def short(v, n=60):
    s = json.dumps(v, ensure_ascii=False)
    return s if len(s) <= n else s[: n - 3] + "..."


def act_text(a):
    rest = {k: v for k, v in a.items() if k != "op"}
    return a["op"] + (" " + " ".join(f"{k}={v}" for k, v in rest.items()) if rest else "")


def write_report(path, name, actions, free, first, local, shots, extra):
    L = [f"# Diff test: {name}", ""]
    L += [f"- actions: {len(actions)} ({act_text(actions[0])})" if actions else "- no actions"]
    L += [f"- {k}: {v}" for k, v in extra.items()]
    L += [""]
    steps_equal = sum(1 for r in free if not r["diffs"])
    L += [f"Free run: {steps_equal} of {len(free)} steps equal; step-local run: "
          f"{sum(1 for r in local if not r['diffs'])} of {len(local)}.", ""]
    L += ["## First divergence (free run)", ""]
    if first is None:
        L += ["None: every step compares equal.", ""]
    else:
        r = free[first]
        L += [f"Step {first}: `{act_text(r['action'])}`", ""]
        L += ["| field | Razdor | original |", "|---|---|---|"]
        for p, a, b in r["diffs"][:40]:
            L += [f"| `{p}` | {short(a)} | {short(b)} |"]
        if len(r["diffs"]) > 40:
            L += [f"| ... {len(r['diffs']) - 40} more | | |"]
        L += [""]
        for s in r["rng"]:
            L += [f"- rng: {s}"]
        for s in r["notes"]:
            L += [f"- {s}"]
        for p, a, b in r["timing"]:
            L += [f"- window timing (gone once the window closes): `{p}` {short(a)} vs {short(b)}"]
        L += ["", f"Screenshots: original `{shots.get('original', '-')}`, Razdor "
              f"`{shots.get('razdor', '-')}` (Razdor's shows the screen without the dialogs "
              "still waiting).", ""]
    for title, rows in (("Per step, free run", free), ("Per step, step-local run "
                        "(each step starts from the original's generator)", local)):
        L += [f"## {title}", "", "| step | action | diffs | fields (first ones) | rng | notes |",
              "|---|---|---|---|---|---|"]
        for r in rows:
            fields = ", ".join(f"`{p}` {short(a, 24)}≠{short(b, 24)}" for p, a, b in r["diffs"][:4])
            if len(r["diffs"]) > 4:
                fields += f", +{len(r['diffs']) - 4}"
            if r.get("timing"):
                fields += ("; " if fields else "") + "window timing: " + ", ".join(
                    f"`{p}`" for p, _, _ in r["timing"][:4])
            rng = "<br>".join(r["rng"][:3])
            L += [f"| {r['step']} | `{act_text(r['action'])}` | {len(r['diffs'])} | {fields} | {rng} | "
                  f"{'<br>'.join(r['notes'])} |"]
        L += [""]
    miss = sorted({(p.split("[")[0], who) for r in free for p, who in r["missing"]})
    L += ["## Fields only one side has", ""]
    L += [f"- `{p}`: only {who}" for p, who in miss] or ["None."]
    L += [""]
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(L))


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--actions", required=True, help="action list v1 (JSON lines)")
    ap.add_argument("--map", help="map file name; puts a new_game first when the list has none")
    ap.add_argument("--hero", type=int, default=1)
    ap.add_argument("--name", help="run folder name (default: list name + time)")
    ap.add_argument("--runs", default=os.path.join(CACHE, "runs"))
    ap.add_argument("--no-build", action="store_true", help="use the last build")
    ap.add_argument("--reuse-original", metavar="DIR",
                    help="take the original's side from an earlier run folder's original/")
    ap.add_argument("--real-music", action="store_true",
                    help="let the original's timed music change draw (real-time noise)")
    ap.add_argument("--no-trace", action="store_true", help="no draw trace in the original")
    ap.add_argument("--trace", metavar="PRESETS",
                    help="Frida runtime trace of the original (tools/difftest/trace.py): "
                         "comma-separated presets or a JSON file of hook specs, written to "
                         "original/trace.jsonl; with `random` Frida gives the draws instead "
                         "of the stub")
    ap.add_argument("--no-shots", action="store_true", help="no Razdor screenshot")
    a = ap.parse_args(argv)

    acts = read_actions(a.actions)
    if a.map and not any(x.get("op") == "new_game" for x in acts):
        acts.insert(0, {"op": "new_game", "map": a.map, "hero": a.hero})
    name = a.name or os.path.splitext(os.path.basename(a.actions))[0] + \
        datetime.datetime.now().strftime("-%Y%m%d-%H%M%S")
    run = os.path.join(a.runs, name)
    os.makedirs(run, exist_ok=True)
    alist = os.path.join(run, "actions.jsonl")
    with open(alist, "w", encoding="utf-8") as f:
        for x in acts:
            f.write(json.dumps(x, ensure_ascii=False) + "\n")

    target = os.path.join(CACHE, "target")
    exe = os.path.join(target, "release", "razdor") if a.no_build else build(target)
    run_razdor(exe, alist, os.path.join(run, "razdor"))
    odir = os.path.join(run, "original")
    if a.reuse_original:
        if os.path.abspath(a.reuse_original) != os.path.abspath(odir):
            shutil.copytree(a.reuse_original, odir, dirs_exist_ok=True)
        orc = 0
    else:
        orc = run_original(alist, odir, a.real_music, not a.no_trace, a.trace)
    orig = load_jsonl(os.path.join(odir, "original.jsonl"))
    orig_run = load_jsonl(os.path.join(odir, "run.jsonl"))
    run_razdor(exe, alist, os.path.join(run, "razdor-sync"), os.path.join(odir, "original.jsonl"))

    raz = load_jsonl(os.path.join(run, "razdor", "razdor.jsonl"))
    raz_run = load_jsonl(os.path.join(run, "razdor", "razdor-run.jsonl"))
    rsync = load_jsonl(os.path.join(run, "razdor-sync", "razdor.jsonl"))
    rsync_run = load_jsonl(os.path.join(run, "razdor-sync", "razdor-run.jsonl"))
    free, first = diff_runs(acts, raz, orig, orig_run, raz_run, not a.real_music)
    local, _ = diff_runs(acts, rsync, orig, orig_run, rsync_run, not a.real_music)

    shots = {}
    if first is not None:
        shots["original"] = os.path.join(odir, f"shot-{first:04d}.png")
        if not a.no_shots:
            p = os.path.join(run, f"razdor-shot-{first:04d}.png")
            if razdor_shot(exe, alist, first, p, run):
                shots["razdor"] = p
    frida_random = bool(a.trace) and "random" in [p.strip() for p in a.trace.split(",")]
    extra = {"run folder": run, "original's exit code": orc,
             "original's music": "real (timed changes draw)" if a.real_music else "held off",
             "original's draw trace": "Frida" if frida_random else "off" if a.no_trace else "stub"}
    if a.trace:
        extra["original's runtime trace"] = f"{a.trace} → {os.path.join(odir, 'trace.jsonl')}"
    write_report(os.path.join(run, "report.md"), name, acts, free, first, local, shots, extra)
    with open(os.path.join(run, "diff.json"), "w", encoding="utf-8") as f:
        json.dump({"first": first, "free": free, "local": local, "shots": shots}, f,
                  ensure_ascii=False, indent=1)
    print(f"report: {os.path.join(run, 'report.md')}")
    print("first divergence:", "none" if first is None else f"step {first}")
    sys.exit(0 if first is None else 1)


if __name__ == "__main__":
    main()
