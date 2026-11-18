import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import expert_iter as ei

GT = "rect 10 10 at=0,0\nextrude 5\n"


def row(ident, images=0, program=GT):
    content = [{"type": "image", "image": f"/x/{ident}_{i}.png"} for i in range(images)]
    content.append({"type": "text", "text": f"make {ident}"})
    return {"id": ident, "messages": [{"role": "user", "content": content}, {"role": "assistant", "content": [{"type": "text", "text": program}]}]}


def s(program, built=True, reward=0.9, stopped=True):
    return {"program": program, "built": built, "reward": reward, "stopped": stopped, "error": None}


class ChooseTargets(unittest.TestCase):
    def test_all_built_keeps_best_own_sample(self):
        chosen = ei.choose_targets([s("a\n", reward=0.8), s("b\n", reward=0.95)], GT)
        self.assertEqual(chosen, ["b\n"])

    def test_failures_keep_two_distinct(self):
        chosen = ei.choose_targets([s("a\n", reward=0.8), s("a\n", reward=0.8), s("b\n", reward=0.9), s("c", built=False, reward=0)], GT)
        self.assertEqual(chosen, ["b\n", "a\n"])

    def test_failures_with_one_good_add_ground_truth(self):
        chosen = ei.choose_targets([s("a\n", reward=0.8), s("c", built=False, reward=0)], GT)
        self.assertEqual(chosen, ["a\n", GT])

    def test_nothing_good_falls_back_to_ground_truth(self):
        chosen = ei.choose_targets([s("a\n", reward=0.3), s("c", built=False, reward=0)], GT)
        self.assertEqual(chosen, [GT])

    def test_truncated_sample_is_not_a_target(self):
        chosen = ei.choose_targets([s("a\n", reward=0.99, stopped=False)], GT)
        self.assertEqual(chosen, [GT])

    def test_fences_are_stripped(self):
        chosen = ei.choose_targets([s("```gcad\nb\n```", reward=0.9)], GT)
        self.assertEqual(chosen, ["b\n"])

    def test_ground_truth_not_duplicated(self):
        chosen = ei.choose_targets([s(GT, reward=1.0), s("c", built=False, reward=0)], GT)
        self.assertEqual(chosen, [GT])


class PickPrompts(unittest.TestCase):
    def test_counts_and_exclusion(self):
        rows = [row(f"b{i}") for i in range(10)] + [row(f"i{i}", images=4) for i in range(10)]
        picked = ei.pick_prompts(rows, 3, 2, seed=1, exclude={("b0", "brief"), ("b1", "brief")})
        self.assertEqual(sum(ei.is_image_row(r) for r in picked), 2)
        self.assertEqual(sum(not ei.is_image_row(r) for r in picked), 3)
        self.assertFalse({"b0", "b1"} & {r["id"] for r in picked})

    def test_only_allowed_ids(self):
        rows = [row("b0"), row("b1"), row("b2")]
        picked = ei.pick_prompts(rows, 3, 0, seed=1, allowed={"b1"})
        self.assertEqual([r["id"] for r in picked], ["b1"])

    def test_same_seed_same_pick(self):
        rows = [row(f"b{i}") for i in range(50)]
        self.assertEqual(ei.pick_prompts(rows, 5, 0, 7), ei.pick_prompts(rows, 5, 0, 7))


class BuildRows(unittest.TestCase):
    def test_rows_keep_prompt_and_replay(self):
        rows = {("b0", "brief"): row("b0"), ("i0", "image"): row("i0", images=4)}
        sampled = [
            {"id": "b0", "kind": "brief", "samples": [s("own\n"), s("x", built=False, reward=0)]},
            {"id": "i0", "kind": "image", "samples": [s("own2\n")]},
        ]
        out, stats = ei.build_rows(sampled, rows, [row("r0")])
        self.assertEqual(stats, {"prompts": 2, "with_failures": 1, "own_targets": 2, "gt_targets": 1, "replay": 1})
        self.assertEqual(len(out), 4)
        self.assertEqual(out[0]["messages"][0], rows[("b0", "brief")]["messages"][0])
        self.assertEqual(out[0]["messages"][1]["content"][0]["text"], "own\n")
        self.assertEqual(out[-1]["id"], "r0")

    def test_same_id_as_brief_and_image_keeps_its_own_prompt(self):
        brief, image = row("p"), row("p", images=4)
        rows = {ei.key(brief): brief, ei.key(image): image}
        sampled = [{"id": "p", "kind": "image", "samples": [s("own\n")]}]
        out, _ = ei.build_rows(sampled, rows, [])
        self.assertTrue(ei.is_image_row(out[0]))

    def test_done_ids_resume(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "s.jsonl"
            self.assertEqual(ei.done_ids(path), set())
            path.write_text(json.dumps({"id": "a", "kind": "brief"}) + "\n" + json.dumps({"id": "a", "kind": "image"}) + "\n")
            self.assertEqual(ei.done_ids(path), {("a", "brief"), ("a", "image")})


class MindsEye(unittest.TestCase):
    def test_refuses_minds_eye_data(self):
        with self.assertRaises(ValueError):
            ei.overlap.refuse_minds_eye(ei.overlap.MINDS_EYE_DIR / "tasks" / "x.jsonl")


if __name__ == "__main__":
    unittest.main()
