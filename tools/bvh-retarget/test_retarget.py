import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent

BVH = """HIERARCHY
ROOT Hips
{
  OFFSET 0 10 0
  CHANNELS 6 Xposition Yposition Zposition Zrotation Yrotation Xrotation
  JOINT Spine
  {
    OFFSET 0 10 0
    CHANNELS 3 Zrotation Yrotation Xrotation
    End Site
    {
      OFFSET 0 10 0
    }
  }
  JOINT Leg
  {
    OFFSET 2 0 0
    CHANNELS 3 Zrotation Yrotation Xrotation
    JOINT Foot
    {
      OFFSET 0 -10 0
      CHANNELS 3 Zrotation Yrotation Xrotation
    }
  }
}
MOTION
Frames: 3
Frame Time: 0.0333333
0 10 0 0 0 0 0 0 0 0 0 0 0 0 0
0 10 0 0 0 0 0 0 0 0 0 45 0 0 0
0 10 0 0 0 0 0 0 0 0 0 90 0 0 0
"""

RIG = """root: blob 0,0,10 r=2
bone hips head=0,0,10 tail=0,0,20
bone leg parent=hips head=2,0,10 tail=2,0,0
"""


def run(config):
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        (tmp / "take.bvh").write_text(BVH)
        (tmp / "rig.gcad").write_text(RIG)
        (tmp / "config.json").write_text(json.dumps(config))
        result = subprocess.run(
            [sys.executable, HERE / "retarget.py", tmp / "config.json", tmp / "out.gcad"],
            capture_output=True,
            text=True,
        )
        out = (tmp / "out.gcad").read_text() if result.returncode == 0 else None
        return result, out


CONFIG = {
    "rig": "rig.gcad",
    "root": "hips",
    "map": {"hips": ["Hips", "Spine"], "leg": ["Leg", "Foot"]},
    "scale": {"bone": "hips", "source": "Hips", "floor": ["Foot"]},
    "facing": ["Leg", "Hips"],
    "contacts": [["leg", "tail", 0.0]],
    "clips": {"kick": {"file": "take.bvh", "lift": "none"}},
}


def test_a_turned_joint_becomes_a_key_of_that_angle():
    result, out = run(CONFIG)
    assert result.returncode == 0, result.stderr
    lines = out.splitlines()
    assert lines[0] == "clip kick fps=30"
    leg = [line for line in lines if "bone=leg" in line]
    assert len(leg) == 3
    angles = [float(line.split("rotate=")[1].split()[0]) for line in leg]
    assert [round(a) for a in angles] == [0, 45, 90], angles
    assert leg[1].startswith("key t=0.0333 bone=leg")
    assert lines[-1] == "key t=0.0667 bone=hips move=0,0,0.000"


def test_a_rig_bone_with_no_source_joint_is_refused():
    config = dict(CONFIG, map={"hips": ["Hips", "Spine"]})
    result, _ = run(config)
    assert result.returncode != 0
    assert "leg" in result.stderr


if __name__ == "__main__":
    for name, test in list(globals().items()):
        if name.startswith("test_"):
            test()
            print("ok", name)
