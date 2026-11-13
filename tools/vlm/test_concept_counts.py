import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import concept_counts

SUMMARY = {"train": {"concept": 1500, "clean": 20}, "val": {"concept": 900, "clean": 20}, "test": {"concept": 880, "clean": 20}}


class ConceptCounts(unittest.TestCase):
    def test_line_reports_each_split(self):
        line = concept_counts.result_line(SUMMARY, 4000, 3280, 720)
        self.assertIn("train 1500, val 900, test 880 (3280 in total)", line)
        self.assertIn("720 dropped", line)

    def test_replaces_only_first_result_line(self):
        doc = "a\nResult: old\nb\n"
        self.assertEqual(concept_counts.replace_result(doc, "Result: new"), "a\nResult: new\nb\n")


if __name__ == "__main__":
    unittest.main()
