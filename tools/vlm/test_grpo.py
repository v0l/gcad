import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import torch

import grpo


class Anchor(unittest.TestCase):
    def test_kl_zero_when_policies_match(self):
        lp = torch.tensor([-1.0, -2.0, -0.5])
        self.assertTrue(torch.allclose(grpo.kl_k3(lp, lp), torch.zeros(3)))

    def test_kl_positive_when_policies_differ(self):
        lp = torch.tensor([-1.0, -2.0])
        ref = torch.tensor([-1.5, -1.0])
        self.assertTrue((grpo.kl_k3(lp, ref) > 0).all())

    def test_beta_adds_penalty_for_drift(self):
        lp = torch.tensor([-1.0, -1.0])
        ref = torch.tensor([-2.0, -2.0])
        free = grpo.sequence_loss(0.0, lp, ref, 0.0)
        anchored = grpo.sequence_loss(0.0, lp, ref, 0.1)
        self.assertGreater(float(anchored), float(free))

    def test_advantages_normalised(self):
        advs = grpo.group_advantages([0.1, 0.5, 0.9, 0.5])
        self.assertAlmostEqual(sum(advs), 0.0, places=5)


if __name__ == "__main__":
    unittest.main()
