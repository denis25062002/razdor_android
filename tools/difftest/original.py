"""Drive the original Discord Times under Wine on a hidden Xvfb display and record its state.

    python -m tools.difftest.original --map "РК1-Начало пути.DTm" --actions run.jsonl --out out/

For every action of the list (action list v1, see README.md) the harness performs it with
XTest mouse clicks and keys, waits until the game settles, then writes
`<out>/state-NNNN.json` (state schema v1, read from memory by memread.py) and
`<out>/shot-NNNN.png`; `<out>/original.jsonl` holds the same states one per line (like
Razdor's `razdor.jsonl`). `<out>/run.jsonl` logs each step with its action and some facts that
are not part of the schema (screen, camera, music timer).

The game runs from a private copy of the install (`--work`, default
~/.cache/razdor-difftest/install): the original folder is never written to. The copy gets
`[Tutorial] Completed=1` so that New game opens the scenario list instead of the tutorial.
Everything the harness starts (Xvfb, the game, and the wineserver when none was running)
is stopped at the end.
"""

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import time

from Xlib import X, XK, display as xdisplay
from Xlib.ext import xtest
from PIL import Image

from . import memread

DEFAULT_INSTALL = os.path.expanduser("~/Games/Discord Times Community Update")
DEFAULT_WORK = os.path.expanduser("~/.cache/razdor-difftest")

# --- pixel layout at 1024x768 (see README.md) ------------------------------------------------
MENU_NEW_GAME = (512, 248)
LIST_X, LIST_TOP, LIST_BOTTOM = 330, 262, 640   # scenario list rows
LIST_SCROLL_DOWN = (497, 634)
NEW_GAME_NEXT = (665, 688)                       # "Далее"
HERO_PORTRAITS = [(315, 325), (512, 325), (709, 325)]
HERO_START = (665, 688)                          # "Старт"
MESSAGE_BOX = (512, 714)                         # hovering it shows the three time buttons
WAIT_1H, WAIT_4H = (418, 713), (607, 713)
VIEW_W, VIEW_H = 1024, 682
CELL_W, CELL_H = 32, 22
SAFE = (48, 40, 976, 640)                        # clickable part of the map view

WINDOWS_CLOSED_BY_ESC = ("building", "village", "shipyard")

KEY_ALIASES = {"esc": "Escape", "escape": "Escape", "enter": "Return", "return": "Return",
               "space": "space", "tab": "Tab", "left": "Left", "right": "Right", "up": "Up",
               "down": "Down", "backspace": "BackSpace"}


class HarnessError(RuntimeError):
    pass


class NotApplicable(HarnessError):
    """The action does not apply now (wrong screen, no question...): noted and skipped."""


def free_display(start=77):
    for n in range(start, start + 100):
        if not os.path.exists(f"/tmp/.X{n}-lock") and not os.path.exists(f"/tmp/.X11-unix/X{n}"):
            return n
    raise HarnessError("no free X display number")


def prepare_install(src, work):
    """Copy the install once into `work/install`, then refresh the ini files from the source
    and patch the tutorial flag. Nothing is deleted."""
    dst = os.path.join(work, "install")
    if not os.path.exists(os.path.join(dst, "DiscordTimes.exe")):
        os.makedirs(work, exist_ok=True)
        shutil.copytree(src, dst, dirs_exist_ok=True)
    for name in os.listdir(src):
        if name.lower().endswith(".ini"):
            shutil.copy2(os.path.join(src, name), os.path.join(dst, name))
    ini = os.path.join(dst, "Rus_DiscordTimes.ini")
    data = open(ini, "rb").read()
    patched = data.replace(b"[Tutorial]\r\nCompleted=0", b"[Tutorial]\r\nCompleted=1")
    if patched == data and b"[Tutorial]\r\nCompleted=1" not in data:
        raise HarnessError("could not set [Tutorial] Completed=1 in the copy")
    open(ini, "wb").write(patched)
    return dst


class Original:
    def __init__(self, install=DEFAULT_INSTALL, work=DEFAULT_WORK, display=None,
                 wineprefix=None, log=None, trace=False, hold_music=False, frida=None,
                 gadget=None):
        self.src = install
        self.work = work
        self.display_num = display
        self.wineprefix = wineprefix
        self.log = log or (lambda *a: print(*a, file=sys.stderr))
        self.xvfb = self.wine = None
        self.started_wineserver = False
        self.pid = None
        self.d = None
        self.game = None
        self.trace_on, self.hold_music = trace, hold_music
        self.trace = None
        self.draws, self.draws_lost = [], 0
        self.frida_presets, self.gadget = frida, gadget
        self.tracer = None

    # --- lifecycle ------------------------------------------------------------------------
    def start(self, timeout=90, tries=2):
        """Start everything and wait for the main menu. A start that hangs (seen once in
        many runs: the game never left its loader) is stopped and tried again."""
        for i in range(tries):
            try:
                return self._start(timeout)
            except HarnessError as e:
                try:
                    self.screenshot(os.path.join(self.work, f"start-fail-{i}.png"))
                except Exception:
                    pass
                if i == tries - 1:
                    raise
                self.log(f"start failed ({e}); trying again")
                self.stop()
                self.pid = self.game = None

    def _start(self, timeout):
        self.dir = prepare_install(self.src, self.work)
        n = self.display_num if self.display_num is not None else free_display()
        self.display = f":{n}"
        self.xvfb = subprocess.Popen(
            ["Xvfb", self.display, "-screen", "0", "1024x768x24", "-nolisten", "tcp"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        for _ in range(100):
            if os.path.exists(f"/tmp/.X11-unix/X{n}"):
                break
            time.sleep(0.05)
        self.d = xdisplay.Display(self.display)
        env = dict(os.environ)
        env.update(DISPLAY=self.display, LANG="ru_RU.UTF-8", LC_ALL="ru_RU.UTF-8",
                   WINEDEBUG="-all", WINEDLLOVERRIDES="winepulse.drv=d",
                   ALSA_CONFIG_PATH=self._null_alsa())
        env.pop("WAYLAND_DISPLAY", None)
        if self.wineprefix:
            env["WINEPREFIX"] = self.wineprefix
        self.env = env
        self.started_wineserver = subprocess.run(
            ["pgrep", "-u", str(os.getuid()), "-x", "wineserver"],
            stdout=subprocess.DEVNULL).returncode != 0
        self.wine_log = open(os.path.join(self.work, "wine.log"), "wb")
        self.wine = subprocess.Popen(["wine", "DiscordTimes.exe"], cwd=self.dir, env=env,
                                     stdout=self.wine_log, stderr=subprocess.STDOUT)
        self.log(f"Xvfb {self.display}, wine pid {self.wine.pid}")
        t0 = time.time()
        while time.time() - t0 < timeout:
            self.pid = self._find_game()
            if self.pid:
                break
            if self.wine.poll() is not None:
                raise HarnessError(f"wine exited early ({self.wine.returncode}); see {self.wine_log.name}")
            time.sleep(0.2)
        else:
            raise HarnessError("the game process did not appear")
        self.game = memread.Game(memread.Memory(self.pid))
        self.wait_screen("main_menu", timeout - (time.time() - t0))
        time.sleep(1.0)  # menu fade-in
        if self.frida_presets:
            from . import trace as frida_trace
            self.tracer = frida_trace.Tracer(self.game.m, self.dir, self.frida_presets,
                                             self.gadget, self.log).start()
            self.trace_on = self.trace_on and not self.traces_random()
        if self.trace_on:
            self.trace = memread.DrawTrace(self.game.m)
            self.trace.install()
        self.log(f"game pid {self.pid}: main menu")

    def traces_random(self):
        """Whether the Frida trace hooks Random (it then gives the draws; the stub is not used)."""
        return bool(self.tracer) and any(s["addr"] == memread.RANDOM for s in self.tracer.specs)

    def tick(self):
        """Collects the traced draws and keeps the timed music change away."""
        if self.trace:
            got, lost = self.trace.poll()
            self.draws += got
            self.draws_lost += lost
        if self.hold_music:
            self.game.hold_music()

    def take_draws(self):
        self.tick()
        out, lost = self.draws, self.draws_lost
        self.draws, self.draws_lost = [], 0
        return out, lost

    def _null_alsa(self):
        path = os.path.join(self.work, "asound-null.conf")
        with open(path, "w") as f:
            f.write("pcm.!default { type null }\nctl.!default { type hw card 0 }\n")
        return path

    def _find_game(self):
        for name in os.listdir("/proc"):
            if not name.isdigit():
                continue
            try:
                cmd = open(f"/proc/{name}/cmdline", "rb").read()
                env = open(f"/proc/{name}/environ", "rb").read()
            except OSError:
                continue
            if b"DiscordTimes.exe" in cmd and f"DISPLAY={self.display}\0".encode() in env:
                try:
                    if memread.Memory(int(name)).read(0x400000, 2) == b"MZ":
                        return int(name)
                except OSError:
                    pass
        return None

    def stop(self):
        if self.tracer:
            self.tracer.stop()
            self.tracer = None
        for pid in {p for p in (self.pid, self.wine and self.wine.pid) if p}:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        for _ in range(50):
            if not (self.pid and os.path.exists(f"/proc/{self.pid}")):
                break
            time.sleep(0.1)
        else:
            try:
                os.kill(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        if self.wine:
            try:
                self.wine.wait(5)
            except subprocess.TimeoutExpired:
                self.wine.kill()
        if self.started_wineserver:
            for args in (["wineserver", "-k"], ["wineserver", "-w"]):  # kill, then wait for it
                try:
                    subprocess.run(args, env=self.env, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, timeout=20)
                except subprocess.TimeoutExpired:
                    pass
        if self.d:
            self.d.close()
            self.d = None
        if self.xvfb:
            self.xvfb.terminate()
            try:
                self.xvfb.wait(5)
            except subprocess.TimeoutExpired:
                self.xvfb.kill()
        self.log("stopped")

    # --- input and screen -----------------------------------------------------------------
    def move(self, x, y):
        xtest.fake_input(self.d, X.MotionNotify, x=int(x), y=int(y))
        self.d.sync()

    def click(self, x, y, button=1, settle=0.12):
        self.move(x, y)
        time.sleep(settle)
        xtest.fake_input(self.d, X.ButtonPress, button)
        self.d.sync()
        time.sleep(0.08)
        xtest.fake_input(self.d, X.ButtonRelease, button)
        self.d.sync()
        time.sleep(0.1)

    def keycode(self, name):
        sym = XK.string_to_keysym(KEY_ALIASES.get(name.lower(), name))
        if not sym and len(name) > 1 and name[0] in "fF" and name[1:].isdigit():
            sym = XK.string_to_keysym(name.upper())
        kc = self.d.keysym_to_keycode(sym) if sym else 0
        if not kc:
            raise NotApplicable(f"unknown key {name!r}")
        return kc

    def key(self, name, hold=0.08):
        kc = self.keycode(name)
        xtest.fake_input(self.d, X.KeyPress, kc)
        self.d.sync()
        time.sleep(hold)
        xtest.fake_input(self.d, X.KeyRelease, kc)
        self.d.sync()

    def screenshot(self, path):
        root = self.d.screen().root
        g = root.get_geometry()
        raw = root.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xFFFFFFFF)
        Image.frombytes("RGB", (g.width, g.height), raw.data, "raw", "BGRX").save(path)

    # --- waiting --------------------------------------------------------------------------
    def wait_screen(self, names, timeout=30):
        names = (names,) if isinstance(names, str) else tuple(names)
        t0 = time.time()
        while time.time() - t0 < timeout:
            if self.game.screen() in names:
                return
            time.sleep(0.05)
        raise HarnessError(f"screen {self.game.screen()} after {timeout}s, wanted {names}")

    def settle(self, quiet=0.6, timeout=180):
        """Wait until the game is at rest: on the world map with an empty timeline, or on
        another window (event, battle, building...), and nothing changed for `quiet` s."""
        g = self.game
        last, since, t0 = None, time.time(), time.time()
        while time.time() - t0 < timeout:
            self.tick()
            scr = g.screen()
            sig = (scr, g.idle(), g.m.i32(memread.TIME_CS), g.rng(), g.dialog_event(),
                   g.m.i32(memread.ARMIES + 0x1724), g.m.i32(memread.ARMIES + 0x1728))
            if scr == "battle":
                # In battle: at rest when the player has the input, the AI's moves and the
                # animations done (after a victory the window stays 2.5 s without input).
                sig += (g.m.u8(memread.INPUT_ON), g.m.read(memread.BATTLE, 0x23))
                at_rest = g.m.u8(memread.INPUT_ON) == 1
            else:
                at_rest = (scr != "world") or g.idle()
            if sig != last:
                last, since = sig, time.time()
            elif at_rest and time.time() - since >= quiet:
                return
            time.sleep(0.05)
        raise HarnessError(f"the game did not settle in {timeout}s ({last})")

    def require(self, *screens):
        scr = self.game.screen()
        if scr not in screens:
            raise NotApplicable(f"on screen {scr}, this action needs {screens}")

    # --- actions (action list v1) ---------------------------------------------------------
    def new_game(self, map_name, hero=None):
        self.require("main_menu")
        self.click(*MENU_NEW_GAME)
        self.wait_screen("new_game")
        time.sleep(0.3)
        entries = self.game.map_list()
        want = match_map(entries, map_name)
        if not want:
            raise HarnessError(f"{map_name} is not in the scenario list: {[e[1] for e in entries]}")
        idx, _, kind = want[0]
        if kind == 2:
            raise HarnessError(f"{map_name} is a later campaign map; the list only starts campaigns "
                               "at their first map")
        self._select_map(idx)
        self.click(*NEW_GAME_NEXT)
        self.wait_screen("new_hero")
        time.sleep(0.3)
        if hero is not None:
            cls = int(hero) - 1
            if not self.game.m.u8(memread.CLASS_ENABLED + cls * 0x4B):
                raise HarnessError(f"hero class {hero} is not offered by {map_name}")
            self.click(*HERO_PORTRAITS[cls])
            time.sleep(0.4)
            if self.game.m.i32(memread.HERO_CLASS) != cls:
                raise HarnessError(f"hero class {hero} did not get selected")
        self.click(*HERO_START)
        self.wait_screen(("world", "event"), timeout=60)
        self.settle(quiet=1.0)

    def _select_map(self, idx):
        sel = lambda: self.game.m.i32(memread.MAP_LIST_SELECTED)
        for page in range(6):
            for y in range(LIST_TOP, LIST_BOTTOM, 9):
                self.click(LIST_X, y, settle=0.05)
                if sel() == idx:
                    return
            for _ in range(8):
                self.click(*LIST_SCROLL_DOWN, settle=0.05)
        raise HarnessError(f"could not select map entry {idx} in the list")

    def cell_pixel(self, x, y):
        cx, cy = self.game.camera()
        return (x + 1) * CELL_W + CELL_W // 2 - cx, (y + 1) * CELL_H + CELL_H // 2 - cy

    def _in_safe(self, px, py):
        x0, y0, x1, y1 = SAFE
        if not (x0 <= px <= x1 and y0 <= py <= y1):
            return False
        mm = self.game.minimap()
        if mm:
            left, top, size = mm
            if left - 16 <= px <= left + size + 16 and py <= top + size + 16:
                return False
        return True

    def scroll_to(self, x, y):
        """Scroll with the arrow keys (one at a time, as the game reads only the last key)
        until cell (x, y) is in the clickable part of the view."""
        self.move(512, 340)
        for _ in range(80):
            px, py = self.cell_pixel(x, y)
            if self._in_safe(px, py):
                return px, py
            before = self.game.camera()
            x0, y0, x1, y1 = SAFE
            mm = self.game.minimap()
            if px < x0 + 100:
                k, d = "Left", (x0 + 200 - px) / 2.0
            elif px > x1 - 100 or (mm and py <= mm[1] + mm[2] + 16 and px >= mm[0] - 16):
                k, d = "Right", (px - (x1 - 200 if not mm else mm[0] - 200)) / 2.0
            elif py < y0 + 80:
                k, d = "Up", (y0 + 160 - py) / 1.375
            else:
                k, d = "Down", (py - (y1 - 160)) / 1.375
            self.key(k, hold=min(0.4, max(0.03, d / 1000.0)))
            time.sleep(0.1)
            if self.game.camera() == before:
                # Clamped: the view cannot move further; accept any on-view position.
                if 5 < px < VIEW_W - 5 and 5 < py < VIEW_H - 5:
                    return px, py
        raise HarnessError(f"could not bring cell {(x, y)} into view")

    def click_map(self, x, y):
        """Go to cell (x, y) the way a player does: a building window still open is closed
        first; the first click plans the route; when the planner reached the cell (it
        becomes the planned target) a second click on it sets off."""
        if self.game.screen() in WINDOWS_CLOSED_BY_ESC:
            self.key("Escape")
            self.settle(quiet=0.3)
        self.require("world")
        self.settle(quiet=0.3)
        px, py = self.scroll_to(x, y)
        self.click(px, py)
        self.settle(quiet=0.3)
        if self.game.screen() == "world" and self.game.idle() and \
                self.game.planned_target() == (x, y) and self.game.hero_cell() != (x, y):
            px, py = self.scroll_to(x, y)
            self.click(px, py)
            self.settle()

    def wait(self, hours):
        self.require("world")
        if hours not in (1, 4):
            raise NotApplicable("wait takes 1 or 4 hours (the two buttons of the original)")
        self.settle(quiet=0.3)
        self.move(*MESSAGE_BOX)
        time.sleep(0.3)
        self.click(*(WAIT_1H if hours == 1 else WAIT_4H))
        self.settle()
        self.move(512, 340)

    def press_key(self, name):
        self.key(name)
        self.settle()

    def _click_widget(self, addr, fallback_key):
        hidden, x, y, w, h = self.game.widget(addr)
        if hidden or w <= 0 or h <= 0:
            if not fallback_key:
                raise NotApplicable("the button is not shown")
            self.key(fallback_key)
        else:
            self.click(x + w // 2, y + h // 2)

    def answer(self, yes):
        scr = self.game.screen()
        if scr == "event":
            # No key fallback for No: in the event window Esc acts as the visible button.
            self._click_widget(memread.EVENT_YES if yes else memread.EVENT_NO,
                               "Return" if yes else None)
        elif scr == "question":
            self.key("Return" if yes else "Escape")
        else:
            raise NotApplicable(f"no question on screen ({scr})")
        self.settle()

    def ok(self):
        """Close the front dialog: the event window's OK, else a building window."""
        scr = self.game.screen()
        if scr == "event":
            self._click_widget(memread.EVENT_OK, "Return")
        elif scr in WINDOWS_CLOSED_BY_ESC:
            self.key("Escape")
        else:
            self.key("Return")
        self.settle()

    def battle_act(self, side, row, col):
        """A press on the battle card of grid cell (row, col) of side 1 (own) or 2 (enemy)."""
        self.require("battle")
        if not self.game.m.u8(memread.INPUT_ON):
            raise NotApplicable("the battle takes no input now")
        rect = self.game.card(side, row, col)
        if rect is None:
            raise NotApplicable(f"no card for side {side} cell {(row, col)}")
        x, y, w, h = rect
        before = self.game.m.read(memread.BATTLE, 0x23 + 2 * memread.SIDE_STRIDE)
        self.click(x + w // 2, y + h // 2)
        time.sleep(0.3)
        self.settle()
        if self.game.screen() == "battle" and \
                self.game.m.read(memread.BATTLE, 0x23 + 2 * memread.SIDE_STRIDE) == before:
            return "no action on that card"
        return None

    def battle_pass(self):
        self.require("battle")
        if not self.game.m.u8(memread.INPUT_ON):
            raise NotApplicable("the battle takes no input now")
        self.key("space")
        time.sleep(0.3)
        self.settle()

    def battle_auto(self):
        # The original has no auto battle (battle.md, interface.md §12): nothing to press.
        return "unsupported: the original has no auto battle"

    def perform(self, act):
        op = act.get("op")
        if op == "new_game":
            self.new_game(act["map"], act.get("hero"))
        elif op == "click_map":
            self.click_map(int(act["x"]), int(act["y"]))
        elif op == "wait":
            self.wait(int(act["hours"]))
        elif op == "key":
            self.press_key(act["key"])
        elif op == "answer":
            self.answer(bool(act["yes"]))
        elif op == "ok":
            self.ok()
        elif op == "battle_auto":
            return self.battle_auto()
        elif op == "battle_act":
            return self.battle_act(int(act["side"]), int(act["row"]), int(act["col"]))
        elif op == "battle_pass":
            self.battle_pass()
        elif op == "snapshot":
            self.settle(quiet=0.3)
        else:
            raise NotApplicable(f"unknown op {op!r}")
        return None

    def map_name(self):
        return self.game.map_file()

    def record(self, out, step, act, note=None):
        st = self.game.state(step, self.map_name())
        with open(os.path.join(out, f"state-{step:04d}.json"), "w") as f:
            json.dump(st, f, ensure_ascii=False)
        with open(os.path.join(out, "original.jsonl"), "a") as f:
            f.write(json.dumps(st, ensure_ascii=False) + "\n")
        self.screenshot(os.path.join(out, f"shot-{step:04d}.png"))
        line = {"step": step, "action": act, "meta": self.game.meta(),
                "now_ms": self.game.m.u32(memread.NOW_MS)}
        if note:
            line["note"] = note
        draws = None
        if self.tracer:
            from . import trace as frida_trace
            recs, dropped = self.tracer.drain()
            with open(os.path.join(out, "trace.jsonl"), "a") as f:
                for r in recs:
                    f.write(json.dumps(r) + "\n")
            if dropped:
                line["trace_dropped"] = dropped
            if self.traces_random():
                draws, lost = frida_trace.draws_from(recs), dropped
        if self.trace:
            draws, lost = self.take_draws()
        if draws is not None:
            line["draws"] = [[n, before, hex(ret), memread.draw_site(ret), t]
                             for n, before, ret, t in draws]
            line["draws_lost"] = lost
        with open(os.path.join(out, "run.jsonl"), "a") as f:
            f.write(json.dumps(line, ensure_ascii=False) + "\n")
        return st


def match_map(entries, name):
    """List entries for `name`: the file name, with or without `.DTm`, or a unique prefix."""
    name = os.path.basename(name).lower()
    exact = [e for e in entries if e[1].lower() in (name, name + ".dtm")]
    if exact:
        return exact
    pre = [e for e in entries if e[1].lower().startswith(name)]
    return pre if len(pre) == 1 else []


def read_actions(path):
    acts = []
    for line in open(path, encoding="utf-8"):
        line = line.strip()
        if line and not line.startswith("#"):
            acts.append(json.loads(line))
    return acts


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--map", help="map file name; prepends a new_game when the list has none")
    ap.add_argument("--hero", type=int, default=1, help="hero class for the prepended new_game")
    ap.add_argument("--actions", help="action list (JSON lines)")
    ap.add_argument("--out", required=True)
    ap.add_argument("--install", default=DEFAULT_INSTALL)
    ap.add_argument("--work", default=DEFAULT_WORK, help="private copy of the install and logs")
    ap.add_argument("--display", type=int, help="X display number (default: first free from 77)")
    ap.add_argument("--wineprefix", help="default: Wine's own (~/.wine)")
    ap.add_argument("--trace-draws", action="store_true",
                    help="log every draw of the generator (a hook on Random; see memread.DrawTrace)")
    ap.add_argument("--trace", metavar="PRESETS",
                    help="Frida runtime trace: comma-separated presets (python -m "
                         "tools.difftest.trace --list) or a JSON file of hook specs; records go "
                         "to <out>/trace.jsonl; with `random` it gives the draws instead of the stub")
    ap.add_argument("--gadget", help="the Windows x86 frida-gadget DLL (see trace.py)")
    ap.add_argument("--hold-music", action="store_true",
                    help="keep the timed music change (a real-time draw) from happening")
    ap.add_argument("--check", action="store_true",
                    help="after the new game, compare memory with the map file and stop")
    a = ap.parse_args(argv)

    acts = read_actions(a.actions) if a.actions else []
    if a.map and not any(x.get("op") == "new_game" for x in acts):
        acts.insert(0, {"op": "new_game", "map": a.map, "hero": a.hero})
    if not acts:
        ap.error("nothing to do: give --map and/or --actions")
    os.makedirs(a.out, exist_ok=True)
    for name in ("run.jsonl", "original.jsonl", "trace.jsonl"):
        open(os.path.join(a.out, name), "w").close()

    o = Original(a.install, a.work, a.display, a.wineprefix,
                 trace=a.trace_draws, hold_music=a.hold_music, frida=a.trace, gadget=a.gadget)
    rc = 0
    try:
        o.start()
        for step, act in enumerate(acts):
            print(f"step {step}: {json.dumps(act, ensure_ascii=False)}", file=sys.stderr)
            if o.tracer:
                o.tracer.set_step(step)
            try:
                note = o.perform(act)
            except NotApplicable as e:
                note = f"skipped: {e}"
                print(f"note: step {step}: {e}", file=sys.stderr)
                o.settle(quiet=0.3)
            o.record(a.out, step, act, note)
            if a.check and act.get("op") == "new_game":
                path = os.path.join(o.dir, "Maps_Rus", o.map_name())
                res = memread.check_against_map(o.game, path)
                bad = [r for r in res if not r[1]]
                for name, ok, detail in bad:
                    print("BAD", name, detail, file=sys.stderr)
                print(f"{len(res) - len(bad)}/{len(res)} checks pass", file=sys.stderr)
                rc = 1 if bad else 0
                break
    except HarnessError as e:
        print(f"error: {e}", file=sys.stderr)
        try:
            o.screenshot(os.path.join(a.out, "error.png"))
        except Exception:
            pass
        rc = 2
    finally:
        o.stop()
    sys.exit(rc)


if __name__ == "__main__":
    main()
