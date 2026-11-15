import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import briefs


class Briefs(unittest.TestCase):
    def test_clean_brief_strips_thinking_quotes_and_rejects_code(self):
        self.assertEqual(briefs.clean_brief('<think>x</think>\n"A 40 mm plate."\n'), "A 40 mm plate.")
        self.assertIsNone(briefs.clean_brief("```\nrect 1 1\n```"))
        self.assertIsNone(briefs.clean_brief("   "))
        self.assertIsNone(briefs.clean_brief("x" * 500))

    def test_caption_sends_the_program_and_cleans_the_reply(self):
        seen = []

        def ask(prompt):
            seen.append(prompt)
            return " A  flat plate,\n 40 mm wide. "

        self.assertEqual(briefs.caption("rect 40 20\nextrude 5\n", ask), "A flat plate, 40 mm wide.")
        self.assertIn("rect 40 20", seen[0])

    def test_text_example_is_text_in_program_out_with_a_stable_style(self):
        a = briefs.text_example("p1", "a plate", "rect 1 1\nextrude 2")
        b = briefs.text_example("p1", "a plate", "rect 1 1\nextrude 2")
        self.assertEqual(a, b)
        user, assistant = a["messages"]
        self.assertEqual([c["type"] for c in user["content"]], ["text"])
        self.assertIn("a plate", user["content"][0]["text"])
        self.assertEqual(assistant["content"][0]["text"], "rect 1 1\nextrude 2\n")

    def test_caption_rows_skips_done_and_failures(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "a.gcad").write_text("rect 1 1\n")
            (root / "b.gcad").write_text("rect 2 2\n")
            rows = [{"id": "a", "gcad": "a.gcad"}, {"id": "b", "gcad": "b.gcad"}]
            got = briefs.caption_rows(rows, root, lambda p: "```", workers=1, done={"a": "kept"})
            self.assertEqual(got, {"a": "kept", "b": None})

    def test_load_done_reads_jsonl(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "b.jsonl"
            path.write_text(json.dumps({"id": "a", "brief": "x"}) + "\n")
            self.assertEqual(briefs.load_done(path), {"a": "x"})
            self.assertEqual(briefs.load_done(Path(tmp) / "none"), {})


if __name__ == "__main__":
    unittest.main()
