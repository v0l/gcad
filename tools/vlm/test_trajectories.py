import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import overlap
import trajectories

TRANSCRIPT = [
    {"role": "system", "content": "sys"},
    {"role": "user", "content": "Make a plate"},
    {"role": "assistant", "content": None, "tool_calls": [
        {"id": "c1", "function": {"name": "gcad", "arguments": "{\"args\": [\"check\", \"model.gcad\"]}"}}]},
    {"role": "tool", "tool_call_id": "c1", "content": "ok"},
    {"role": "user", "content": [{"type": "image", "image": "/tmp/runs/a/0.png"}, {"type": "text", "text": "Here is the image."}]},
    {"role": "assistant", "content": "done"},
]
BANNED = {"names": {"fox"}, "prompts": [set(overlap.words("A red fox game character sitting looking forward"))]}


def run(brief="a flat plate with four holes", built=True, reward=0.8, transcript=TRANSCRIPT):
    return {"id": "r1", "brief": brief, "built": built, "reward": reward, "transcript": transcript}


class Trajectories(unittest.TestCase):
    def test_keep_run_needs_build_and_reward(self):
        self.assertTrue(trajectories.keep_run(run()))
        self.assertFalse(trajectories.keep_run(run(built=False)))
        self.assertFalse(trajectories.keep_run(run(reward=0.2)))

    def test_example_keeps_tool_calls_results_and_images(self):
        ex = trajectories.trajectory_example("r1", TRANSCRIPT)
        self.assertEqual([m["role"] for m in ex["messages"]], ["user", "assistant", "tool", "user", "assistant"])
        self.assertEqual(ex["messages"][1]["tool_calls"][0]["name"], "gcad")
        self.assertEqual(ex["messages"][3]["content"][0], {"type": "image", "image": "/tmp/runs/a/0.png"})

    def test_collect_drops_low_reward_and_minds_eye_overlap(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "runs.jsonl"
            rows = [run(), run(reward=0.1), run(brief="a red fox game character")]
            path.write_text("\n".join(json.dumps(r) for r in rows) + "\n")
            self.assertEqual(len(trajectories.collect(path, BANNED)), 1)

    def test_collect_refuses_a_minds_eye_run_folder(self):
        with self.assertRaises(ValueError):
            trajectories.collect(overlap.MINDS_EYE_DIR / "results" / "runs" / "x.jsonl", BANNED)

    def test_example_with_minds_eye_image_is_dropped(self):
        image = str(overlap.MINDS_EYE_DIR / "results" / "runs" / "fox-s0" / "images" / "0.png")
        transcript = [TRANSCRIPT[0], TRANSCRIPT[1], {"role": "user", "content": [{"type": "image_url", "image_url": {}}]}]
        transcript[2] = {"role": "user", "content": [{"type": "image", "image": image}]}
        ex = trajectories.trajectory_example("r1", transcript)
        self.assertFalse(trajectories.clean_example(ex))


if __name__ == "__main__":
    unittest.main()
