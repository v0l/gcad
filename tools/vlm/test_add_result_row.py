import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import add_result_row

DOC = "| a | 1 |\n| SFT then GRPO, 120 steps | 89% | 7% | 0.46 | 1.55% | 22.6% |\n\ntext\n"


def result(built, chamfer, volume, reward):
    r = {"built": built, "reward": reward}
    if built:
        r["metrics"] = {"chamfer": chamfer, "volume_error": volume}
    return r


class ResultRow(unittest.TestCase):
    def write(self, rows):
        path = Path(tempfile.mkdtemp()) / "r.jsonl"
        path.write_text("\n".join(json.dumps(r) for r in rows) + "\n")
        return path

    def test_summary_counts_build_and_match(self):
        path = self.write([result(True, 0.001, 0.0, 1.0), result(True, 0.02, 0.2, 0.3), result(False, 0, 0, 0.0), result(False, 0, 0, 0.0)])
        s = add_result_row.summarise_file(path)
        self.assertEqual((s["build"], s["match"]), (0.5, 0.25))
        self.assertAlmostEqual(s["reward"], 0.325)

    def test_row_inserted_after_grpo_row_and_replaced_on_rerun(self):
        row = "| GRPO v2 | 90% | 20% | 0.52 | 0.80% | 10.0% |"
        once = add_result_row.insert_row(DOC, row)
        twice = add_result_row.insert_row(once, row)
        self.assertEqual(once, twice)
        lines = once.splitlines()
        self.assertEqual(lines[2], row)


if __name__ == "__main__":
    unittest.main()
