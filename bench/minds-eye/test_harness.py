import os
import shutil
import tempfile
import unittest

import harness
import metrics
from agent_api import silhouette_overlap

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


class Tasks(unittest.TestCase):
    def test_sets_have_expected_sizes_and_are_disjoint(self):
        tuning = harness.load_tasks(["tuning"])
        held = harness.load_tasks(["heldout"])
        self.assertGreaterEqual(len(tuning), 30)
        self.assertGreaterEqual(len(held), 10)
        self.assertFalse({t["id"] for t in tuning} & {t["id"] for t in held})

    def test_every_task_has_rubric_and_fields(self):
        for t in harness.load_tasks(["tuning", "heldout"]):
            self.assertGreaterEqual(len(t["rubric"]), 4, t["id"])
            for k in ("id", "domain", "kind", "prompt", "min_parts"):
                self.assertIn(k, t)
            for b in t.get("bbox", []):
                self.assertIn(b["num"], "xyz")
                self.assertLess(b["min"], b["max"])

    def test_domains_cover_the_brief(self):
        domains = {t["domain"] for t in harness.load_tasks(["tuning"])}
        self.assertEqual(domains, {"creature", "prop", "vehicle", "mechanical", "building"})


class Metrics(unittest.TestCase):
    def test_op_usage_ignores_labels_and_let(self):
        ops = metrics.op_usage("let a=1\ntorso: blob 0,0,0 r=1\nsmooth iterations=2\nmirror x of=torso")
        self.assertEqual(ops, {"blob": 1, "smooth": 1, "mirror": 1})
        self.assertTrue(metrics.mesh_op_report(ops)["beyond_primitives"])
        self.assertFalse(metrics.mesh_op_report({"blob": 3})["beyond_primitives"])

    def test_bbox_checks(self):
        out = metrics.bbox_checks({"x": 2.0, "y": 1.0, "z": 1.0}, [{"num": "x", "den": "y", "min": 1.5, "max": 2.5},
                                                                  {"num": "z", "den": "y", "min": 2, "max": 3}])
        self.assertEqual([b["pass"] for b in out], [True, False])

    def test_complexity_of_square_is_16(self):
        import numpy as np

        m = np.zeros((100, 100), bool)
        m[10:90, 10:90] = True
        self.assertAlmostEqual(metrics.complexity(m), 16, delta=1.0)

    def test_pearson_and_ranks(self):
        import harness as h

        self.assertEqual(h.pearson([1, 2, 3], [2, 4, 6]), 1.0)
        self.assertEqual(h.ranks([5, 1, 5]), [2.5, 1.0, 2.5])

    def test_sandbox_blocks_escape(self):
        from agent_api import Sandbox

        d = tempfile.mkdtemp()
        box = Sandbox(d, False, False)
        with self.assertRaises(ValueError):
            box.path("../x")
        self.assertIn("not available", box.gcad(["render", "a", "b"]))


@unittest.skipUnless(shutil.which("gcad"), "gcad not on PATH")
class Built(unittest.TestCase):
    def test_fox_measures(self):
        d = tempfile.mkdtemp()
        task = {"id": "fox", "kind": "mesh", "min_parts": 8, "symmetric": True,
                "bbox": [{"num": "y", "den": "z", "min": 0.6, "max": 1.8}]}
        r = metrics.measure(os.path.join(REPO, "examples", "mesh", "fox.gcad"), task, d)
        self.assertTrue(r["builds"])
        self.assertGreater(r["triangles"], 10000)
        self.assertGreater(r["named_parts"], 20)
        self.assertTrue(r["beyond_primitives"])
        self.assertGreater(r["hard_score"], 5)

    def test_broken_model_scores_zero(self):
        d = tempfile.mkdtemp()
        p = os.path.join(d, "model.gcad")
        open(p, "w").write("bogus 1\n")
        r = metrics.measure(p, {"kind": "mesh", "min_parts": 1}, d)
        self.assertFalse(r["builds"])
        self.assertEqual(r["hard_score"], 0.0)

    def test_render_overlap_with_itself_is_one(self):
        d = tempfile.mkdtemp()
        png = os.path.join(d, "r.png")
        metrics.gcad("render", os.path.join(REPO, "examples", "mesh", "fox.gcad"), png)
        self.assertGreater(silhouette_overlap(png, png)["iou"], 0.99)


class Agreement(unittest.TestCase):
    def test_stats_split_rated_and_decent_runs(self):
        rows = [(2, 3.0, "a"), (2, 5.0, "b"), ("decent", 6.0, "c"), ("decent", 2.0, "d")]
        out = harness.agreement_stats(rows)
        self.assertEqual(out["n_rated"], 2)
        self.assertEqual(out["n_decent"], 2)
        self.assertEqual(out["mean_abs_error_rated"], 2.0)
        self.assertEqual(out["within_1_rated"], 0.0)
        self.assertEqual(out["decent_above_rated_auc"], 0.5)


if __name__ == "__main__":
    unittest.main()
