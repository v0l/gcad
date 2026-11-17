import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import mix_data


class MixData(unittest.TestCase):
    def test_image_share_and_determinism(self):
        briefs = [{"id": f"b{i}"} for i in range(80)]
        images = [{"id": f"i{i}"} for i in range(100)]
        a = mix_data.mix(briefs, images, 0.2, 1)
        self.assertEqual(sum(1 for r in a if r["id"].startswith("i")), 20)
        self.assertEqual(len(a), 100)
        self.assertEqual(a, mix_data.mix(briefs, images, 0.2, 1))

    def test_small_image_pool_is_used_whole(self):
        a = mix_data.mix([{"id": "b"}] * 10, [{"id": "i"}], 0.5, 1)
        self.assertEqual(len(a), 11)


if __name__ == "__main__":
    unittest.main()
