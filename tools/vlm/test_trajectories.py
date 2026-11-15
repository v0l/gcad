import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import trajectories

TRANSCRIPT = [
    {"role": "system", "content": "skill"},
    {"role": "user", "content": "Request: a chest"},
    {"role": "assistant", "content": None, "tool_calls": [
        {"id": "c1", "function": {"name": "gcad", "arguments": "{\"args\": [\"check\", \"model.gcad\"]}"}}]},
    {"role": "tool", "tool_call_id": "c1", "content": "ok"},
    {"role": "user", "content": [{"type": "image", "image": "images/0.png"}, {"type": "text", "text": "Here is the image."}]},
    {"role": "assistant", "content": "done"},
]


def row(builds=True, hard=7.0, judge=6.0, task="chest"):
    return {"task": task, "seed": 0, "hard": {"builds": builds, "hard_score": hard}, "judge": {"overall": judge}}


class Trajectories(unittest.TestCase):
    def test_keeps_only_runs_that_build_and_score_well(self):
        self.assertTrue(trajectories.keep_row(row()))
        self.assertFalse(trajectories.keep_row(row(builds=False)))
        self.assertFalse(trajectories.keep_row(row(hard=3.0)))
        self.assertFalse(trajectories.keep_row(row(judge=2.0)))
        self.assertFalse(trajectories.keep_row({"task": "x", "hard": {"builds": True, "hard_score": 9}, "judge": None}))

    def test_example_keeps_tool_calls_results_and_images(self):
        ex = trajectories.trajectory_example("chest-s0", TRANSCRIPT)
        self.assertEqual(ex["system"], "skill")
        roles = [m["role"] for m in ex["messages"]]
        self.assertEqual(roles, ["user", "assistant", "tool", "user", "assistant"])
        self.assertEqual(ex["messages"][1]["tool_calls"][0]["name"], "gcad")
        self.assertEqual(ex["messages"][2]["tool_call_id"], "c1")
        self.assertEqual(ex["messages"][3]["content"][0], {"type": "image", "image": "images/0.png"})
        self.assertEqual(ex["messages"][0]["content"], [{"type": "text", "text": "Request: a chest"}])

    def test_collect_reads_logs_of_kept_rows(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            results = root / "r.json"
            results.write_text(json.dumps({"rows": [row(), row(builds=False, task="fox")]}))
            run = root / "runs" / "chest-s0"
            run.mkdir(parents=True)
            (run / "agent.jsonl").write_text(json.dumps({"assistant": "x"}) + "\n" + json.dumps({"transcript": TRANSCRIPT}) + "\n")
            got = trajectories.collect(results, root / "runs")
            self.assertEqual([g["id"] for g in got], ["chest-s0"])


if __name__ == "__main__":
    unittest.main()
