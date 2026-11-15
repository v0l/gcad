import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import minds_eye_row

DOC = (
    "text\n\n"
    + minds_eye_row.HEADER
    + "\n|---|---|---|---|---|---|---|---|---|\n"
    "| gcadvlm-render (SFT 4B) | 16 | 6% | 1.0 | 0.00 | 0.34 | 5k | 187 | 0-1 vs local |\n"
    "| local-render (qwen3.8-27b + skill) | 15 | 47% | 4.9 | 0.81 | 3.88 | 714k | 683 | reference |\n"
    "\nafter\n"
)

SUMMARY = {"runs": 16, "build_rate": 0.875, "judge_overall": 6.25, "rubric_fraction": 0.9, "hard_score": 4.1, "tokens": 1234567, "seconds": 812.4}


class MindsEyeRow(unittest.TestCase):
    def test_row_format(self):
        row = minds_eye_row.format_row("opus-render", SUMMARY)
        self.assertEqual(row, "| opus-render | 16 | 88% | 6.2 | 0.90 | 4.10 | 1235k | 812 | not run |")

    def test_row_appended_at_end_of_table_and_replaced_on_rerun(self):
        row = minds_eye_row.format_row("opus-render", SUMMARY)
        once = minds_eye_row.insert_row(DOC, row)
        twice = minds_eye_row.insert_row(once, row)
        self.assertEqual(once, twice)
        lines = once.splitlines()
        self.assertEqual(lines[6], row)
        self.assertEqual(lines[8], "after")

    def test_pairwise_cell(self):
        import json
        import tempfile
        path = os.path.join(tempfile.mkdtemp(), "p.json")
        json.dump({"wins_a": 9, "wins_b": 5, "ties": 2}, open(path, "w"))
        self.assertEqual(minds_eye_row.pairwise_cell(path, "local"), "9-5 vs local, 2 ties")
        self.assertEqual(minds_eye_row.pairwise_cell("", "local"), "not run")

    def test_missing_judge_leaves_cells_blank(self):
        summary = dict(SUMMARY, judge_overall=None, rubric_fraction=None)
        row = minds_eye_row.format_row("x", summary)
        self.assertIn("| 88% |  |  | 4.10 |", row)


if __name__ == "__main__":
    unittest.main()
