import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import adapter_dir


class AdapterDir(unittest.TestCase):
    def test_flat_adapter_returned_as_is(self):
        d = Path(tempfile.mkdtemp())
        (d / "adapter_config.json").write_text("{}")
        self.assertEqual(adapter_dir.adapter_dir(d), d)

    def test_grpo_output_resolves_to_policy_subdirectory(self):
        d = Path(tempfile.mkdtemp())
        (d / "policy").mkdir()
        (d / "policy" / "adapter_config.json").write_text("{}")
        self.assertEqual(adapter_dir.adapter_dir(d), d / "policy")

    def test_missing_adapter_raises(self):
        with self.assertRaises(FileNotFoundError):
            adapter_dir.adapter_dir(tempfile.mkdtemp())

    def test_real_grpo_output_if_present(self):
        real = Path.home() / "gcad-datasets/out/vlm/lora_grpo2"
        if not real.exists():
            self.skipTest("no grpo output")
        self.assertTrue((adapter_dir.adapter_dir(real) / "adapter_config.json").exists())


if __name__ == "__main__":
    unittest.main()
