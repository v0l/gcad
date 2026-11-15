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


class FullSequence(unittest.TestCase):
    def test_per_token_inputs_extended_over_the_completion(self):
        batch = {
            "input_ids": torch.tensor([[1, 2, 3, 4]]),
            "attention_mask": torch.ones(1, 4, dtype=torch.long),
            "mm_token_type_ids": torch.tensor([[0, 1, 1, 0]]),
            "pixel_values": torch.zeros(8, 3),
            "image_grid_thw": torch.tensor([[1, 2, 4]]),
        }
        completion = torch.tensor([[9, 9, 7]])
        full = grpo.full_sequence(batch, completion)
        self.assertEqual(full["input_ids"].tolist(), [[1, 2, 3, 4, 9, 9, 7]])
        self.assertEqual(full["attention_mask"].tolist(), [[1] * 7])
        self.assertEqual(full["mm_token_type_ids"].tolist(), [[0, 1, 1, 0, 0, 0, 0]])
        self.assertIs(full["pixel_values"], batch["pixel_values"])
        self.assertIs(full["image_grid_thw"], batch["image_grid_thw"])

    def test_without_mm_token_type_ids(self):
        batch = {"input_ids": torch.tensor([[1, 2]]), "attention_mask": torch.ones(1, 2, dtype=torch.long)}
        full = grpo.full_sequence(batch, torch.tensor([[5]]))
        self.assertEqual(set(full), {"input_ids", "attention_mask"})


if __name__ == "__main__":
    unittest.main()
