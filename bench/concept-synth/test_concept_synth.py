import json
import os
import tempfile
import unittest

import numpy as np
from PIL import Image

import filter_iou
import pick as selector


def disc(path, cx, cy, r, bg=(236, 236, 232), fg=(200, 80, 60), alpha=False, size=256):
    yy, xx = np.mgrid[:size, :size]
    inside = (xx - cx) ** 2 + (yy - cy) ** 2 < r * r
    if alpha:
        a = np.zeros((size, size, 4), np.uint8)
        a[inside] = fg + (255,)
    else:
        a = np.zeros((size, size, 3), np.uint8)
        a[:] = bg
        a[inside] = fg
    Image.fromarray(a).save(path)


class FilterTests(unittest.TestCase):
    def test_identical_silhouette_scores_near_one(self):
        with tempfile.TemporaryDirectory() as d:
            disc(d + "/r.png", 128, 128, 70, alpha=True)
            disc(d + "/c.png", 128, 128, 70)
            self.assertGreater(filter_iou.score(d + "/c.png", d + "/r.png"), 0.95)

    def test_shifted_small_object_scores_low(self):
        with tempfile.TemporaryDirectory() as d:
            disc(d + "/r.png", 128, 128, 70, alpha=True)
            disc(d + "/c.png", 128, 128, 20)
            s = filter_iou.score(d + "/c.png", d + "/r.png")
            self.assertLess(s, 0.2)

    def test_area_ratio_gives_iou(self):
        with tempfile.TemporaryDirectory() as d:
            disc(d + "/r.png", 128, 128, 80, alpha=True)
            disc(d + "/c.png", 128, 128, 40)
            s = filter_iou.score(d + "/c.png", d + "/r.png")
            self.assertAlmostEqual(s, 0.25, delta=0.04)

    def test_filter_logs_drops(self):
        with tempfile.TemporaryDirectory() as d:
            disc(d + "/r1.png", 128, 128, 70, alpha=True)
            disc(d + "/c1.png", 128, 128, 70)
            disc(d + "/r2.png", 128, 128, 70, alpha=True)
            disc(d + "/c2.png", 40, 40, 20)
            with open(d + "/g.jsonl", "w") as f:
                f.write(json.dumps({"id": "a", "concept": d + "/c1.png", "render": d + "/r1.png"}) + "\n")
                f.write(json.dumps({"id": "b", "concept": d + "/c2.png", "render": d + "/r2.png"}) + "\n")
            kept, dropped = filter_iou.filter_generated(d + "/g.jsonl", 0.75, d + "/k", d + "/x")
            self.assertEqual([g["id"] for g in kept], ["a"])
            self.assertEqual([g["id"] for g in dropped], ["b"])
            self.assertIn("silhouette_iou", dropped[0]["drop_reason"])
            self.assertEqual(len(open(d + "/x").read().splitlines()), 1)


class SelectTests(unittest.TestCase):
    def rec(self, i, **kw):
        m = {"id": f"s:{i}", "source": "s", "commercial_ok": True, "gcad_fit": "high", "has_concept": False,
             "triangles": 1000, "paths": {"glb": "x.glb"}}
        m.update(kw)
        return m

    def test_filters_and_determinism(self):
        recs = [self.rec(i) for i in range(10)]
        recs.append(self.rec(20, commercial_ok=False))
        recs.append(self.rec(21, gcad_fit="low"))
        recs.append(self.rec(22, has_concept=True))
        a = selector.select(recs, 5, 1)
        b = selector.select(recs, 5, 1)
        self.assertEqual(a, b)
        self.assertEqual(len(a), 5)
        self.assertTrue(all(m["commercial_ok"] and m["gcad_fit"] == "high" and not m["has_concept"] for m in a))


class MergeTests(unittest.TestCase):
    def test_synthetic_and_real_kinds(self):
        import merge_manifest
        with tempfile.TemporaryDirectory() as d:
            real = {"id": "r:1", "has_concept": True, "paths": {"concept": ["c.png"]}}
            plain = {"id": "p:1", "has_concept": False, "paths": {"glb": "x.glb"}}
            other = {"id": "p:2", "has_concept": False, "paths": {}}
            with open(d + "/m", "w") as f:
                for m in (real, plain, other):
                    f.write(json.dumps(m) + "\n")
            gen = {"id": "p:1", "concept": d + "/c.png", "generator": "g", "prompt": "p", "silhouette_iou": 0.9}
            with open(d + "/k", "w") as f:
                f.write(json.dumps(gen) + "\n")
            self.assertEqual(merge_manifest.merge(d + "/m", d + "/k", d + "/o"), (1, 1))
            out = [json.loads(l) for l in open(d + "/o")]
            self.assertEqual(out[0]["concept_kind"], "real")
            self.assertTrue(out[1]["has_concept"])
            self.assertEqual(out[1]["concept_kind"], "synthetic")
            self.assertEqual((out[1]["generator"], out[1]["prompt"]), ("g", "p"))
            self.assertEqual(len(out[1]["paths"]["concept"]), 1)
            self.assertNotIn("concept_kind", out[2])
            self.assertFalse(out[2]["has_concept"])


class AblationTests(unittest.TestCase):
    def test_training_fits_a_linear_target(self):
        import ablation
        rng = np.random.default_rng(0)
        x = rng.normal(size=(64, 5))
        w_true = rng.normal(size=(5, 3))
        y = x @ w_true
        w = ablation.train(x, y, 3000, 16, 0, lr=0.1, l2=0.0)
        self.assertLess(ablation.error(w, x, y), 0.05)

    def test_arms_use_same_objects(self):
        import ablation
        with tempfile.TemporaryDirectory() as d:
            disc(d + "/r.png", 128, 128, 70, alpha=True)
            disc(d + "/c.png", 128, 128, 70)
            rec = {"id": "a", "bbox_min": [0, 0, 0], "bbox_max": [1, 2, 3], "parts": 1}
            synth = [{"render": d + "/r.png", "concept": d + "/c.png", "record": rec}]
            xa, _ = ablation.arm_data("A", synth, [])
            xb, _ = ablation.arm_data("B", synth, [])
            xc, _ = ablation.arm_data("C", synth, [])
            self.assertEqual((len(xa), len(xb), len(xc)), (1, 2, 0))


if __name__ == "__main__":
    unittest.main()
