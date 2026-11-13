import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pick_pairs


def row(i, ops, split="train"):
    return {"id": f"zero-to-cad:{i:08x}-0000", "source": "zero-to-cad", "split": split, "commercial_ok": True, "ops": ops}


class PickPairs(unittest.TestCase):
    def test_stratified_keeps_op_mix(self):
        rows = [row(i, ["extrude", "rect"]) for i in range(80)] + [row(100 + i, ["extrude", "hole", "rect"]) for i in range(20)]
        chosen = pick_pairs.stratified(rows, 10, 1)
        self.assertEqual(len(chosen), 10)
        holes = [r for r in chosen if "hole" in r["ops"]]
        self.assertEqual(len(holes), 2)

    def test_select_includes_all_val_and_test(self):
        train = [row(i, ["extrude"]) for i in range(50)]
        val = [row(1000 + i, ["extrude"], "val") for i in range(3)]
        test = [row(2000 + i, ["extrude"], "test") for i in range(4)]
        records = pick_pairs.select(train, val, test, 10, 1)
        self.assertEqual(len(records), 17)
        self.assertEqual(records[0]["paths"]["glb"].split("/")[1], "zero-to-cad")

    def test_deterministic(self):
        train = [row(i, ["extrude"] if i % 3 else ["hole"]) for i in range(60)]
        a = pick_pairs.select(train, [], [], 20, 5)
        b = pick_pairs.select(train, [], [], 20, 5)
        self.assertEqual(a, b)


if __name__ == "__main__":
    unittest.main()
