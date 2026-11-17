import shutil
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import infer
import repair

GOOD = "rect 20 10 at=0,0\nextrude 5\n"


def fake_build(program):
    for number, line in enumerate(program.splitlines(), 1):
        if line.startswith("bad"):
            return f"Error: line {number}: {line}: nope"
    return None if program.strip() else "Error: the file builds no solid"


class FailingLine(unittest.TestCase):
    def test_reads_line_number(self):
        self.assertEqual(repair.failing_line("Error: line 7: hole 4: cut failed"), 7)

    def test_none_without_number(self):
        self.assertIsNone(repair.failing_line("timeout"))

    def test_drop_line(self):
        self.assertEqual(repair.drop_line("a\nb\nc\n", 2), "a\nc\n")
        self.assertIsNone(repair.drop_line("a\n", 5))


class Repair(unittest.TestCase):
    def test_single_shot_built(self):
        r = repair.repair(GOOD, lambda: self.fail("no resample"), 3, build=fake_build)
        self.assertEqual((r["stage"], r["tries"]), ("single_shot", 1))

    def test_resample_fixes(self):
        samples = iter(["bad\n", GOOD])
        r = repair.repair("bad\n", lambda: next(samples), 3, build=fake_build)
        self.assertEqual((r["stage"], r["tries"], r["built"]), ("resample", 3, True))

    def test_drop_lines_when_samples_fail(self):
        r = repair.repair(GOOD + "bad 1\nbad 2\n", lambda: "bad\n", 2, build=fake_build)
        self.assertEqual((r["stage"], r["drops"], r["built"]), ("drop_lines", 2, True))
        self.assertEqual(r["program"], GOOD)

    def test_fences_stripped(self):
        r = repair.repair("```gcad\n" + GOOD + "```", lambda: "", 1, build=fake_build)
        self.assertTrue(r["built"])

    def test_stub_when_everything_dropped(self):
        r = repair.repair("bad\n", lambda: "bad\n", 1, build=lambda p: None if p == repair.STUB else "Error: line 1: x")
        self.assertEqual(r["stage"], "stub")


class Infer(unittest.TestCase):
    def test_resamples_drawn_once_in_a_batch(self):
        calls = []

        def generate(n, temperature):
            calls.append((n, temperature))
            return [GOOD] if temperature == 0 else ["bad\n"] * n

        self.assertEqual(infer.solve(generate, 8)["stage"], "single_shot")
        self.assertEqual(calls, [(1, 0.0)])

    def test_failed_greedy_takes_first_building_sample(self):
        calls = []

        def generate(n, temperature):
            calls.append((n, temperature))
            return ["bad\n"] if temperature == 0 else ["bad\n", GOOD, "bad\n"]

        r = repair.repair(generate(1, 0)[0], infer.batch_resampler(lambda: generate(3, 0.8)), 4, build=fake_build)
        self.assertEqual((r["stage"], r["tries"]), ("resample", 3))
        self.assertEqual(calls, [(1, 0), (3, 0.8)])

    def test_resampler_empty_batch_gives_empty_program(self):
        self.assertEqual(infer.batch_resampler(lambda: [])(), "")


@unittest.skipUnless(shutil.which(repair.GCAD), "gcad binary not on PATH")
class RealGcad(unittest.TestCase):
    def test_builds_and_reports(self):
        self.assertIsNone(repair.build_error(GOOD))
        self.assertIn("line 1", repair.build_error("nonsense 1\n"))

    def test_drop_loop_with_real_gcad(self):
        program, error, drops = repair.drop_failing_lines(GOOD + "nonsense 1\n")
        self.assertEqual((error, drops), (None, 1))


if __name__ == "__main__":
    unittest.main()
