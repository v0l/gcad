import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import merge_results


def row(task, seed, judge):
    return {"task": task, "seed": seed, "judge": {"overall": judge, "rubric_fraction": 1.0},
            "hard": {"builds": True, "hard_score": 5.0}, "run": {"tokens": {"input": 10, "output": 5}, "seconds": 3.0, "tool_calls": 2}}


class Merge(unittest.TestCase):
    def test_keeps_only_chosen_tasks_and_later_parts_win(self):
        a = {"config": {"name": "x"}, "rows": [row("fox", 0, 5), row("owl", 0, 9), row("sword", 0, 1)]}
        b = {"config": {"name": "x"}, "rows": [row("sword", 0, 7), row("sword", 1, 3)]}
        out = merge_results.merge([a, b], ["fox", "sword"])
        self.assertEqual([(r["task"], r["seed"]) for r in out["rows"]], [("fox", 0), ("sword", 0), ("sword", 1)])
        self.assertEqual(out["rows"][1]["judge"]["overall"], 7)
        self.assertEqual(out["summary"]["runs"], 3)
        self.assertAlmostEqual(out["summary"]["judge_overall"], 5.0)


if __name__ == "__main__":
    unittest.main()
