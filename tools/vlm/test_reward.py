import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import reward

BOX = "rect 20 10\nextrude 5\n"
BIG_BOX = "rect 40 30\nextrude 25\n"


@unittest.skipUnless(shutil.which(reward.GCAD), "gcad binary not on PATH")
class Reward(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.mkdtemp()
        cls.ref = Path(cls.tmp) / "ref.stl"
        src = Path(cls.tmp) / "ref.gcad"
        src.write_text(BOX)
        subprocess.run([reward.GCAD, "export", str(src), str(cls.ref)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp)

    def test_identical_program_gets_full_reward(self):
        r = reward.score(BOX, self.ref)
        self.assertTrue(r["built"])
        self.assertAlmostEqual(r["reward"], 1.0, places=2)

    def test_fenced_program_is_accepted(self):
        r = reward.score("```gcad\n" + BOX + "```", self.ref)
        self.assertGreater(r["reward"], 0.99)

    def test_wrong_shape_scores_below_right_shape_but_above_unbuilt(self):
        right = reward.score(BOX, self.ref)["reward"]
        wrong = reward.score(BIG_BOX, self.ref)
        self.assertTrue(wrong["built"])
        self.assertLess(wrong["reward"], right)
        self.assertGreaterEqual(wrong["reward"], 0.1)

    def test_program_that_does_not_build_gets_zero(self):
        r = reward.score("frobnicate 3\n", self.ref)
        self.assertFalse(r["built"])
        self.assertEqual(r["reward"], 0.0)
        self.assertTrue(r["error"])

    def test_strip_fences(self):
        self.assertEqual(reward.strip_fences("```\na\nb\n```"), "a\nb\n")
        self.assertEqual(reward.strip_fences("a\n"), "a\n")


if __name__ == "__main__":
    unittest.main()
