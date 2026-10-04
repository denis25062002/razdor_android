"""Forced coverage for the explorer (`explore.py --cover`): episodes built around one action kind.

Each episode takes a required kind (the one played least on the original side so far), steers
to a place where it is valid with scripted setup steps (played on both sides like any other
action), lets the model pick one action of that kind among the valid ones (a scripted pick
when its answer is no use), then lets the model play a few more actions as usual.

Setups, all decided on Razdor's replay before anything is played (a candidate is simulated
first and only played when it reaches the place):
- a building whose window offers the kind: the nearest buildings are tried in turn (click,
  messages closed with OK, questions answered no) until one shows it (`goods`, `sell`,
  `recruits`, heal or raise prices, unknown spells, affordable);
- an item in the pack (sell, equip): the cheapest item of the nearest market is bought (for
  equip one a unit can wear, else the loot of a won fight);
- a spell in the book (cast): the cheapest unknown spell of the nearest sanctuary is learnt;
- a wounded unit (heal) or a dead one (resurrect): a fight with a weak hostile army or a
  ruins' garrison; the battle is searched on Razdor's replay (seeded mixes of passes and
  strikes) for a won one that leaves a unit wounded or dead, and that line of presses is
  played; a hero alone hires one unit first;
- a question (answer): the nearest building or event whose click ends on a question;
- a battle (battle_act, battle_pass): a click on a weak hostile army.
"""

import random

from . import explore as E

# The action kinds the coverage counts, in the order of the table.
KINDS = ("click_map", "wait", "ok", "answer", "battle_act", "battle_pass", "buy", "sell", "hire",
         "heal", "resurrect", "learn", "cast", "equip")
# The kinds an episode can be built around.
FORCED = ("sell", "heal", "resurrect", "equip", "buy", "hire", "learn", "cast", "answer",
          "battle_act", "battle_pass", "wait")
HOSTILE = (3, 4)   # army factions a fight is picked from (neighbour, enemy)


def gold(st):
    return st["hero"]["gold"]


def units_hp(st):
    return [u["hp"] for u in st["hero"]["units"]]


def choices(kind, st, screen, look):
    """The actions of `kind` that make sense now (Razdor may still refuse some)."""
    if st is None:
        return []
    g = gold(st)
    if kind == "wait":
        return [{"op": "wait", "hours": h} for h in (1, 4)] if screen == "map" else []
    if kind == "ok":
        return [{"op": "ok"}] if screen in ("dialog", "building") else []
    if kind == "answer":
        return [{"op": "answer", "yes": y} for y in (True, False)] if screen in ("question", "offer") else []
    if kind in ("battle_act", "battle_pass"):
        if screen != "battle":
            return []
        if kind == "battle_pass":
            return [{"op": "battle_pass"}]
        b = st["battle"]
        return [{"op": "battle_act", "side": side + 1, "row": u["row"], "col": u["col"]}
                for side in (1, 0) for u in b["sides"][side]]
    if kind in ("cast", "equip"):
        if screen not in ("map", "building"):
            return []
        if kind == "equip":
            return [{"op": "equip", "slot": p["slot"], "unit": u}
                    for p in look.get("pack", []) for u in range(len(st["hero"]["units"]))]
        mana = st["hero"]["mana"]
        out = []
        for b in look.get("book", []):
            if b["mana"] > mana:
                continue
            if b["enemy"]:
                out += [{"op": "cast", "slot": b["slot"], "army": a["id"]} for a in st.get("armies", [])
                        if a.get("active") and a.get("alive", True) and "x" in a]
            else:
                out.append({"op": "cast", "slot": b["slot"]})
        return out
    if screen != "building":
        return []
    if kind == "buy":
        return [{"op": "buy", "slot": x["slot"]} for x in look.get("goods") or [] if x["price"] <= g]
    if kind == "sell":
        return [{"op": "sell", "slot": x["slot"]} for x in look.get("sell") or []]
    if kind == "hire":
        return [{"op": "hire", "slot": x["slot"]} for x in look.get("recruits") or [] if x["price"] <= g]
    if kind in ("heal", "resurrect"):
        return [{"op": kind, "unit": x["unit"]} for x in look.get("services") or []
                if x["op"] == kind and x["price"] <= g]
    if kind == "learn":
        return [{"op": "learn", "slot": x["slot"]} for x in look.get("spells") or []
                if not x["known"] and x["price"] <= g]
    return []


class Cover:
    """The setup and the forced pick of one kind in an `explore.Episode`."""

    def __init__(self, ep, kind, rollouts=10):
        self.ep, self.kind, self.rollouts = ep, kind, rollouts
        self.rnd = random.Random(ep.rnd.random())
        self.steps = []   # what the setup did, for the log

    # --- Razdor-only simulation -------------------------------------------------------------
    def sim(self, extra):
        return self.ep.razdor.look(self.ep.acts + extra)

    def settle_sim(self, extra, stop=("map", "building", "battle", "ended"), answer=False, limit=12):
        """`extra` and the OKs (and no-answers) that close what it opens, on Razdor only:
        (the actions, state, screen, look)."""
        extra = list(extra)
        st, screen, look = self.sim(extra)
        for _ in range(limit):
            if screen in stop or st is None:
                break
            if screen == "dialog":
                extra.append({"op": "ok"})
            elif screen in ("question", "offer"):
                extra.append({"op": "answer", "yes": answer})
            else:
                break
            st, screen, look = self.sim(extra)
        return extra, st, screen, look

    def play(self, extra, tag=" (cover setup)"):
        """Plays `extra` on both sides (it was simulated)."""
        for a in extra:
            self.ep.visit(a)
            self.ep.push(a, tag)
            if self.ep.screen == "ended":
                return False
        return True

    def close_windows(self):
        """OK on messages, no on questions, until a map, building or battle shows."""
        for _ in range(12):
            if self.ep.screen == "dialog":
                self.ep.push({"op": "ok"}, " (cover setup)")
            elif self.ep.screen in ("question", "offer"):
                self.ep.push({"op": "answer", "yes": False}, " (cover setup)")
            else:
                return

    # --- steering ---------------------------------------------------------------------------
    def hero_cell(self):
        h = self.ep.state["hero"]
        return h["x"], h["y"]

    def buildings_near(self, n=10):
        hp = self.hero_cell()
        bs = [b for b in self.ep.info.m.buildings if b.type not in (13, 14)]
        bs.sort(key=lambda b: E.dist(hp, (b.x, b.y)))
        return bs[:n]

    def hop_toward(self, target):
        """One walk that brings the hero closer to `target` (walking explores the map): the
        reachable cell nearest to it that opens no battle. False when none is closer."""
        ep = self.ep
        hp = self.hero_cell()
        cells = list(E.sample_cells(ep.info, hp)) + [(b.x, b.y) for b in self.buildings_near(16)]
        ok = ep.razdor.reachable(ep.acts, cells)
        for c in sorted(ok, key=lambda c: E.dist(c, target))[:6]:
            if E.dist(c, target) >= E.dist(hp, target):
                break
            a = {"op": "click_map", "x": c[0], "y": c[1]}
            if ep.too_strong(a):
                continue
            extra, st, screen, look = self.settle_sim([a])
            if screen in ("map", "building") and st and (st["hero"]["x"], st["hero"]["y"]) != hp:
                self.steps.append(f"walk toward {target}: {c}")
                return self.play(extra)
        return False

    def to_building(self, pred, what, hops=8):
        """Walks into the nearest building whose window satisfies `pred(state, look)`,
        trying the reachable buildings nearest first and walking toward the nearest one not
        tried yet when none does."""
        ep = self.ep
        tried = set()
        for _ in range(hops + 1):
            if ep.screen == "building" and pred(ep.state, ep.look):
                return True
            self.close_windows()
            bs = self.buildings_near(16)
            ok = ep.razdor.reachable(ep.acts, [(b.x, b.y) for b in bs if b.id not in tried])
            for b in bs:
                if b.id in tried or (b.x, b.y) not in ok:
                    continue
                tried.add(b.id)
                extra, st, screen, look = self.settle_sim([{"op": "click_map", "x": b.x, "y": b.y}])
                if screen == "building" and pred(st, look):
                    self.steps.append(f"{what}: building {b.id} at ({b.x},{b.y})")
                    return self.play(extra)
            left = [b for b in bs if b.id not in tried]
            if not left or not self.hop_toward((left[0].x, left[0].y)):
                break
        self.steps.append(f"{what}: none found")
        return False

    def wearable(self, extra, item_slot):
        """Whether some unit can wear pack item `item_slot` after `extra` (Razdor only)."""
        ep = self.ep
        n = len(ep.state["hero"]["units"])
        base = ep.acts + extra
        for u in range(n):
            _, notes, _ = ep.razdor.replay(base + [{"op": "equip", "slot": item_slot, "unit": u}])
            if not notes.get(len(base)):
                return True
        return False

    def ensure_pack(self, wear=False):
        """An item in the pack (one a unit can wear when `wear`): the cheapest fitting item
        of the nearest market is bought."""
        ep = self.ep
        pack = ep.look.get("pack") or []
        if pack and (not wear or any(self.wearable([], p["slot"]) for p in pack)):
            return True

        def market(st, look):
            return any(x["price"] <= gold(st) for x in look.get("goods") or [])
        if not self.to_building(market, "market for an item"):
            return False
        goods = sorted((x for x in ep.look.get("goods") or [] if x["price"] <= gold(ep.state)),
                       key=lambda x: x["price"])
        for x in goods[:12]:
            buy = {"op": "buy", "slot": x["slot"]}
            if ep.note_of(buy):
                continue
            if wear and not self.wearable([buy], len(pack)):
                continue
            ep.push(buy, " (cover setup)")
            return bool(ep.look.get("pack"))
        self.steps.append("no item to buy" + (" that a unit can wear" if wear else ""))
        return False

    def raise_gold(self, need):
        """Sells pack items here, the dearest first, until the gold reaches `need`."""
        ep = self.ep
        while gold(ep.state) < need and ep.screen == "building":
            rows = sorted(ep.look.get("sell") or [], key=lambda x: -x["price"])
            if not rows:
                return False
            ep.push({"op": "sell", "slot": rows[0]["slot"]}, " (cover setup: gold)")
            self.steps.append(f"sold item {rows[0]['id']} for gold")
        return gold(ep.state) >= need

    def ensure_book(self):
        ep = self.ep
        mana = ep.state["hero"]["mana"]
        if any(b["mana"] <= mana for b in ep.look.get("book", [])):
            return True

        def sanctuary(st, look):
            return any(not x["known"] and x["price"] <= gold(st) for x in look.get("spells") or [])
        if not self.to_building(sanctuary, "sanctuary for a spell"):
            return False
        cheap = min((x for x in ep.look["spells"] if not x["known"] and x["price"] <= gold(ep.state)),
                    key=lambda x: x["price"])
        ep.push({"op": "learn", "slot": cheap["slot"]}, " (cover setup)")
        return any(b["mana"] <= ep.state["hero"]["mana"] for b in ep.look.get("book", []))

    def foes(self):
        """Cells of weak hostile armies and of ruins not the hero's, nearest first."""
        ep = self.ep
        st = ep.state
        hp = self.hero_cell()
        mine = E.strength(st["hero"]["units"])
        out = []
        for a in st.get("armies", []):
            if not (a.get("active") and a.get("alive", True) and "x" in a):
                continue
            if ep.info.army_faction.get(a["id"]) not in HOSTILE:
                continue
            if E.strength(a.get("units")) > mine * E.STRONGER or (a["x"], a["y"]) in ep.avoid:
                continue
            out.append((E.dist(hp, (a["x"], a["y"])), (a["x"], a["y"])))
        owners = {b["id"]: b.get("owner") for b in st.get("buildings", [])}
        for b in ep.info.m.buildings:
            if b.type == 12 and owners.get(b.id) != 0:
                out.append((E.dist(hp, (b.x, b.y)), (b.x, b.y)))
        out.sort()
        return [c for _, c in out[:8]]

    def to_battle(self, hops=6):
        """Simulated actions that open a battle with a weak foe: (extra, state) or None. The
        hero walks toward the nearest foe while none is in reach."""
        ep = self.ep
        for _ in range(hops + 1):
            self.close_windows()
            cells = self.foes()
            if not cells:
                return None
            ok = ep.razdor.reachable(ep.acts, cells)
            for c in cells:
                if c not in ok:
                    continue
                extra, st, screen, look = self.settle_sim([{"op": "click_map", "x": c[0], "y": c[1]}],
                                                          answer=True)
                if screen == "battle":
                    return extra, st
            if not self.hop_toward(cells[0]):
                return None
        return None

    def win_battle(self, extra, need):
        """Seeded rollouts of the battle `extra` opened, on Razdor only: the first won one
        that leaves a unit `need` ('wounded' or 'dead'). Returns the actions or None."""
        ep = self.ep
        before = units_hp(ep.state)
        for r in range(self.rollouts):
            rnd = random.Random(self.rnd.random())
            p = (0.15, 0.4, 0.65)[r % 3]
            line = list(extra)
            st, screen, _ = self.sim(line)
            while screen == "battle" and len(line) < len(extra) + 150:
                if rnd.random() < p:
                    a = {"op": "battle_pass"}
                else:
                    a = E.fallback(ep.info, st, screen, rnd, ep.visited, ep.razdor, ep.acts + line)
                line.append(a)
                st, screen, _ = self.sim(line)
            line, st, screen, _ = self.settle_sim(line, stop=("map", "building", "ended", "battle"))
            if st is None or screen in ("ended", "battle"):
                continue
            after = units_hp(st)
            k = min(len(before), len(after))
            dead = any(after[i] <= 0 < before[i] for i in range(k))
            hurt = any(0 < after[i] < before[i] for i in range(k))
            if need == "won" or (need == "dead" and dead) or (need == "wounded" and (hurt or dead)):
                self.steps.append(f"battle won with a unit {need} (rollout {r}, {len(line)} actions)")
                return line
        return None

    def ensure_hurt(self, need):
        """A wounded or dead unit (`need`; 'won': any) after a won fight."""
        ep = self.ep
        if need == "dead" and len(ep.state["hero"]["units"]) < 2:
            # The hero alone: a unit to lose first.
            def barracks(st, look):
                return any(x["price"] <= gold(st) for x in look.get("recruits") or [])
            if not self.to_building(barracks, "barracks for a unit to lose"):
                return False
            cheap = min((x for x in ep.look["recruits"] if x["price"] <= gold(ep.state)), key=lambda x: x["price"])
            ep.push({"op": "hire", "slot": cheap["slot"]}, " (cover setup)")
        got = self.to_battle()
        if not got:
            self.steps.append("no weak foe in reach")
            return False
        line = self.win_battle(got[0], need)
        if not line:
            self.steps.append(f"no won battle with a unit {need} in {self.rollouts} rollouts")
            return False
        return self.play(line)

    def setup(self):
        """Steers to a place where the kind is valid. Returns True when it is."""
        ep, kind = self.ep, self.kind
        self.close_windows()
        if choices(kind, ep.state, ep.screen, ep.look):
            return True
        if kind in ("buy", "hire", "learn", "heal", "resurrect", "sell"):
            if kind == "sell" and not self.ensure_pack():
                return False
            if kind in ("heal", "resurrect"):
                # A place first (so that the price is known), then the wound.
                if not self.ensure_hurt("dead" if kind == "resurrect" else "wounded"):
                    return False
                self.close_windows()
            if choices(kind, ep.state, ep.screen, ep.look):
                return True
            if kind in ("heal", "resurrect"):
                # Offered is enough: items sold here pay for it when the gold falls short.
                def offered(st, look):
                    return any(x["op"] == kind for x in look.get("services") or []) and \
                        (any(x["op"] == kind and x["price"] <= gold(st) for x in look["services"])
                         or bool(look.get("sell")))
                if not self.to_building(offered, kind):
                    return False
                price = min(x["price"] for x in ep.look["services"] if x["op"] == kind)
                return self.raise_gold(price) and bool(choices(kind, ep.state, ep.screen, ep.look))
            return self.to_building(lambda st, look: bool(choices(kind, st, "building", look)), kind)
        if kind == "equip":
            if not self.ensure_pack(wear=True):
                # Nothing wearable for sale: a won fight's loot.
                self.close_windows()
                if not self.ensure_hurt("won"):
                    return False
                if not any(self.wearable([], p["slot"]) for p in ep.look.get("pack") or []):
                    self.steps.append("the loot is nothing a unit can wear")
                    return False
            self.close_windows()
            return bool(choices(kind, ep.state, ep.screen, ep.look))
        if kind == "cast":
            if not self.ensure_book():
                return False
            return bool(choices(kind, ep.state, ep.screen, ep.look))
        if kind == "wait":
            if ep.screen == "building":
                ep.push({"op": "ok"}, " (cover setup)")
            return ep.screen == "map"
        if kind == "answer":
            for b in self.buildings_near():
                extra, st, screen, look = self.settle_sim([{"op": "click_map", "x": b.x, "y": b.y}],
                                                          stop=("map", "building", "battle", "ended", "question", "offer"))
                if screen in ("question", "offer"):
                    self.steps.append(f"question at building {b.id}")
                    return self.play(extra)
            return False
        if kind in ("battle_act", "battle_pass"):
            got = self.to_battle()
            if not got:
                self.steps.append("no weak foe in reach")
                return False
            self.play(got[0])
            return ep.screen == "battle"
        return False

    def pick(self):
        """One action of the kind: the model's choice when Razdor takes it, else a scripted
        one. Returns (the action or None, 'model' / 'script')."""
        ep, kind = self.ep, self.kind
        valid = choices(kind, ep.state, ep.screen, ep.look)
        extra = (f"COVERAGE TEST: this turn reply with exactly one action of op \"{kind}\" "
                 f"(choose which one yourself), e.g. {E.act_text(valid[0]) if valid else kind}.")
        cands = [a for a in ep.ask_model(f"Use the action '{kind}' now.", extra=extra) if a["op"] == kind]
        for a in cands:
            if not ep.note_of(a):
                ep.st["accepted_from_model"] += 1
                ep.push(a, " (cover pick)")
                return a, "model"
        self.rnd.shuffle(valid)
        for a in valid[:8]:
            if not ep.note_of(a):
                ep.push(a, " (cover pick, scripted)")
                return a, "script"
        return None, "none"


def run_cover(ep, kind, goals, tail):
    """Setup, the forced pick, then `tail` more actions by the model. Returns the cover
    record for `log.jsonl` and why the episode stopped."""
    c = Cover(ep, kind)
    rec = {"kind": kind, "reached": False, "by": None, "step": None, "action": None}
    try:
        reached = c.setup()
    except Exception as e:   # a setup that breaks is logged, the episode goes on
        c.steps.append(f"setup error: {e}")
        reached = False
    rec["setup"] = c.steps
    rec["setup_len"] = len(ep.acts)
    if reached and ep.screen != "ended":
        rec["reached"] = True
        a, by = c.pick()
        rec["by"] = by
        if a:
            rec["step"] = len(ep.acts) - 1
            rec["action"] = a
            rec["original_note"] = ep.original_notes[-1] if ep.live else "no original"
    why = ep.explore(goals, len(ep.acts) + tail)
    return rec, why


class _Silent:
    """A model that answers nothing (the dry setup only steers)."""
    calls = 0
    seconds = 0.0

    def ask(self, prompt):
        return "{}"


def dry_setup(info, hero, razdor, kind, seed):
    """Whether the setup for `kind` reaches it on `info` with `hero`, on Razdor's replay
    alone (the same seed gives the same steering when it is played for real)."""
    ep = E.Episode(info, hero, razdor, _Silent(), random.Random(seed), lambda r: None)
    if ep.start():
        return False
    try:
        return Cover(ep, kind).setup()
    except Exception:
        return False
