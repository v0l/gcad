import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import pairs_data


def fake_dataset(root):
    (root / "g").mkdir()
    (root / "g" / "a.gcad").write_text("rect 10 10\nextrude 5\n")
    renders = {}
    for view in pairs_data.VIEWS:
        path = root / f"a_{view}.png"
        path.write_bytes(b"x")
        renders[view] = path.name
    rows = [
        {"id": "a", "split": "train", "commercial_ok": True, "gcad": "g/a.gcad", "renders": renders, "lines": 2, "ops": ["rect", "extrude"]},
        {"id": "b", "split": "train", "commercial_ok": False, "gcad": "g/a.gcad", "renders": renders, "lines": 2, "ops": ["rect"]},
        {"id": "c", "split": "test", "commercial_ok": True, "gcad": "g/a.gcad", "renders": renders, "lines": 90, "ops": ["rect"]},
    ]
    pairs = root / "pairs.jsonl"
    pairs.write_text("\n".join(json.dumps(r) for r in rows) + "\n")
    return pairs


class PairsData(unittest.TestCase):
    def test_filters_split_licence_and_length(self):
        with tempfile.TemporaryDirectory() as tmp:
            pairs = fake_dataset(Path(tmp))
            self.assertEqual([r["id"] for r in pairs_data.read_pairs(pairs, split="train")], ["a"])
            self.assertEqual(len(pairs_data.read_pairs(pairs, split="train", commercial_only=False)), 2)
            self.assertEqual(pairs_data.read_pairs(pairs, split="test", max_lines=60), [])

    def test_chat_example_has_four_images_then_prompt_then_program(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            row = pairs_data.read_pairs(fake_dataset(root), split="train")[0]
            ex = pairs_data.chat_example(row, data=root)
            user, assistant = ex["messages"]
            kinds = [c["type"] for c in user["content"]]
            self.assertEqual(kinds, ["image"] * 4 + ["text"])
            self.assertEqual(assistant["content"][0]["text"], "rect 10 10\nextrude 5\n")

    def test_op_histogram_counts_programs(self):
        rows = [{"ops": ["rect", "extrude"]}, {"ops": ["rect"]}]
        self.assertEqual(pairs_data.op_histogram(rows), {"rect": 2, "extrude": 1})


if __name__ == "__main__":
    unittest.main()
