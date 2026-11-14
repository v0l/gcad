import json
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
DOC = "docs/vlm-finetune.md"
TABLE = "| model | builds |\n|---|---|\n| SFT then GRPO, 120 steps | 89% | 7% | 0.46 | 1.55% | 22.6% |\n| other | 1 |\n"


def git(cwd, *args):
    return subprocess.run(["git", "-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@t", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout


def results_file(directory, name, reward):
    path = Path(directory) / name
    path.write_text(json.dumps({"built": True, "reward": reward, "metrics": {"chamfer": 0.001, "volume_error": 0.0}}) + "\n")
    return str(path)


class PublishRows(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        git(self.root, "init", "-q", "--bare", "-b", "master", "origin.git")
        seed = self.root / "seed"
        git(self.root, "clone", "-q", "origin.git", "seed")
        (seed / "docs").mkdir()
        (seed / DOC).write_text(TABLE)
        git(seed, "add", ".")
        git(seed, "commit", "-qm", "init")
        git(seed, "push", "-q", "origin", "HEAD:master")
        self.clones = []
        for name in ("a", "b"):
            git(self.root, "clone", "-q", "origin.git", name)
            self.clones.append(self.root / name)

    def publish(self, clone, label, reward, out):
        results = results_file(self.root, label.replace(" ", "_") + ".jsonl", reward)
        out.append(subprocess.run(["bash", str(HERE / "publish_rows.sh"), str(clone), DOC, "rows", sys.executable, results, label], capture_output=True, text=True))

    def test_two_rows_added_concurrently_both_land(self):
        out = []
        threads = [threading.Thread(target=self.publish, args=(self.clones[0], "row alpha", 0.9, out)), threading.Thread(target=self.publish, args=(self.clones[1], "row beta", 0.8, out))]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        self.assertTrue(all(r.returncode == 0 for r in out), [r.stderr for r in out])
        git(self.root / "seed", "pull", "-q", "origin", "master")
        doc = (self.root / "seed" / DOC).read_text()
        self.assertIn("| row alpha |", doc)
        self.assertIn("| row beta |", doc)
        self.assertNotIn("<<<<<<<", doc)

    def test_stale_clone_publishes_on_top_of_new_origin(self):
        out = []
        self.publish(self.clones[0], "row one", 0.9, out)
        self.publish(self.clones[1], "row two", 0.8, out)
        self.assertTrue(all(r.returncode == 0 for r in out), [r.stderr for r in out])
        git(self.root / "seed", "pull", "-q", "origin", "master")
        doc = (self.root / "seed" / DOC).read_text()
        self.assertIn("| row one |", doc)
        self.assertIn("| row two |", doc)


if __name__ == "__main__":
    unittest.main()
