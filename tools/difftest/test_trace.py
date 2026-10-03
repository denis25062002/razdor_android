"""Checks of the Frida trace that need no running game.

    ~/.local/opt/re-venv/bin/python -m unittest tools.difftest.test_trace
"""

import json
import shutil
import subprocess
import unittest

from . import trace


class LoaderStub(unittest.TestCase):
    def test_disassembles_to_the_one_shot_loader(self):
        try:
            import capstone
        except ImportError:
            self.skipTest("capstone not installed")
        md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
        code = [f"{i.mnemonic} {i.op_str}".strip() for i in md.disasm(trace.loader_stub(), trace.LOADER)]
        self.assertEqual(code, [
            "pushal", "pushfd",
            f"mov dword ptr [{trace.LOADER_ENTERED:#x}], 1",
            f"mov eax, dword ptr [{trace.LOADER_SAVED:#x}]",
            f"mov dword ptr [{trace.IAT_TIMEGETTIME:#x}], eax",
            f"push {trace.LOADER_NAME:#x}",
            f"call dword ptr [{trace.IAT_LOADLIBRARYA:#x}]",
            f"mov dword ptr [{trace.LOADER_RESULT:#x}], eax",
            "popfd", "popal",
            f"jmp dword ptr [{trace.IAT_TIMEGETTIME:#x}]"])
        self.assertLess(trace.LOADER + len(trace.loader_stub()), trace.LOADER_SAVED)


class Presets(unittest.TestCase):
    def test_every_preset_builds(self):
        for name in trace.PRESETS:
            specs = trace.specs_for(name)
            self.assertTrue(specs, name)
            for s in specs:
                self.assertTrue(0x401000 <= s["addr"] < 0x4E8000, (name, s["name"]))
                for arg in s.get("args", []):
                    self.assertIn(arg[1].split(":")[0], ("eax", "edx", "ecx", "stack"))

    def test_army_filter_and_one_hook_per_address(self):
        specs = trace.specs_for("ai:1,ai_frames:3")
        clocks = [s for s in specs if s["addr"] == 0x4A399C]
        self.assertEqual(len(clocks), 1)
        self.assertEqual(clocks[0]["when"], "a.k == 3")     # the later spec wins
        self.assertNotIn("log_if", clocks[0])
        plan = next(s for s in specs if s["name"] == "AiPlan")
        self.assertEqual(plan["when"], "a.k == 1")

    def test_unknown_preset(self):
        with self.assertRaises(ValueError):
            trace.specs_for("random,nope")

    def test_draws_from_records(self):
        recs = [{"f": "Random", "a": {"n": 40}, "e": {"before": 7}, "ra": 0x4A2594, "t": 100},
                {"f": "AiPlan", "a": {"k": 1}, "ra": 0, "t": 100}]
        self.assertEqual(trace.draws_from(recs), [(40, 7, 0x4A2594, 100)])


@unittest.skipUnless(shutil.which("node"), "node not installed")
class AgentSyntax(unittest.TestCase):
    def test_agent_and_preset_expressions_parse(self):
        subprocess.run(["node", "--check", trace.AGENT], check=True)
        exprs = []
        for name in trace.PRESETS:
            for s in trace.specs_for(name):
                for key in ("when", "log_if"):
                    if s.get(key):
                        exprs.append(s[key])
                for key in ("enter", "leave"):
                    exprs += list((s.get(key) or {}).values())
        js = ("const exprs = " + json.dumps(exprs) + ";\n"
              "for (const x of exprs) new Function('a','e','l','r','i32','u32','i16','u16','i8','u8',"
              "'army','t', 'return (' + x + ');');\n")
        subprocess.run(["node", "-e", js], check=True)


if __name__ == "__main__":
    unittest.main()
