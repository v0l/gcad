import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import dpo
from test_expert_iter import GT, row, s


class PromptPairs(unittest.TestCase):
    def test_best_built_against_each_distinct_failure(self):
        entry = {"samples": [s("a\n", reward=0.7), s("b\n", reward=0.9), s("x\n", built=False, reward=0), s("x\n", built=False, reward=0), s("y\n", built=False, reward=0)]}
        self.assertEqual(dpo.prompt_pairs(entry, GT), [("b\n", "x\n"), ("b\n", "y\n")])

    def test_no_failures_no_pairs(self):
        self.assertEqual(dpo.prompt_pairs({"samples": [s("a\n")]}, GT), [])

    def test_weak_built_uses_ground_truth(self):
        entry = {"samples": [s("a\n", reward=0.2), s("x\n", built=False, reward=0)]}
        self.assertEqual(dpo.prompt_pairs(entry, GT), [(GT, "x\n")])

    def test_truncated_built_is_not_chosen(self):
        entry = {"samples": [s("a\n", reward=0.99, stopped=False), s("x\n", built=False, reward=0)]}
        self.assertEqual(dpo.prompt_pairs(entry, GT), [(GT, "x\n")])

    def test_stopped_failures_first(self):
        entry = {"samples": [s("long\n", built=False, reward=0, stopped=False), s("x\n", built=False, reward=0), s("b\n")]}
        self.assertEqual(dpo.prompt_pairs(entry, GT, per_prompt=1), [("b\n", "x\n")])


class BuildPairs(unittest.TestCase):
    def test_pairs_share_the_prompt(self):
        image = row("p", images=4)
        rows = {("p", "image"): image}
        sampled = [{"id": "p", "kind": "image", "samples": [s("b\n"), s("x\n", built=False, reward=0)]}]
        pairs = dpo.build_pairs(sampled, rows)
        self.assertEqual(len(pairs), 1)
        self.assertEqual(pairs[0]["chosen"]["messages"][0], image["messages"][0])
        self.assertEqual(pairs[0]["rejected"]["messages"][0], image["messages"][0])
        self.assertEqual(pairs[0]["rejected"]["messages"][1]["content"][0]["text"], "x\n")


class Loss(unittest.TestCase):
    def test_loss_falls_as_margin_grows(self):
        import torch

        t = torch.tensor
        low, m_low = dpo.preference_loss(t(-10.0), t(-10.0), t(-10.0), t(-10.0), t(10), beta=0.1, alpha=0.0)
        high, m_high = dpo.preference_loss(t(-5.0), t(-20.0), t(-10.0), t(-10.0), t(10), beta=0.1, alpha=0.0)
        self.assertAlmostEqual(m_low.item(), 0.0)
        self.assertAlmostEqual(m_high.item(), 15.0)
        self.assertLess(high.item(), low.item())

    def test_nll_term_is_per_token(self):
        import torch

        t = torch.tensor
        base, _ = dpo.preference_loss(t(-10.0), t(-10.0), t(-10.0), t(-10.0), t(10), beta=0.1, alpha=0.0)
        with_nll, _ = dpo.preference_loss(t(-10.0), t(-10.0), t(-10.0), t(-10.0), t(10), beta=0.1, alpha=1.0)
        self.assertAlmostEqual(with_nll.item() - base.item(), 1.0, places=5)


if __name__ == "__main__":
    unittest.main()
