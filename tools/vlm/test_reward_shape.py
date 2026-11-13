import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import reward


class RewardShape(unittest.TestCase):
    def metrics(self, chamfer, volume_error):
        return {"volume_a": 1.0, "volume_b": 1.0, "chamfer": chamfer, "volume_error": volume_error}

    def test_reward_falls_as_chamfer_grows(self):
        values = [reward.reward_from_metrics(self.metrics(c, 0.0)) for c in (0.0, 0.01, 0.03, 0.2)]
        self.assertEqual(values, sorted(values, reverse=True))
        self.assertAlmostEqual(values[0], 1.0)
        self.assertAlmostEqual(values[-1], 0.1)

    def test_volume_error_lowers_reward(self):
        good = reward.reward_from_metrics(self.metrics(0.0, 0.0))
        bad = reward.reward_from_metrics(self.metrics(0.0, 0.5))
        self.assertLess(bad, good)

    def test_empty_volume_gets_zero(self):
        m = self.metrics(0.0, 0.0)
        m["volume_b"] = 0.0
        self.assertEqual(reward.reward_from_metrics(m), 0.0)


if __name__ == "__main__":
    unittest.main()
