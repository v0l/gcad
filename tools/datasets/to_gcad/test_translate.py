import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import cadquery as real_cq

from shape_distance import compare_meshes, load_mesh, verdict
from translate import Unsupported, ops_of, translate

PROGRAMS = {
    "rect_extrude": (
        "import cadquery as cq\nresult = cq.Workplane('XY').rect(40, 20).extrude(8)\n",
        {"rect", "extrude"},
    ),
    "circle_extrude": (
        "import cadquery as cq\nresult = cq.Workplane('XY').circle(12).extrude(9)\n",
        {"circle", "extrude"},
    ),
    "polygon": (
        "import cadquery as cq\nresult = cq.Workplane('XY').polygon(6, 30).extrude(7)\n",
        {"ngon", "extrude"},
    ),
    "polyline": (
        "import cadquery as cq\n"
        "result = cq.Workplane('XY').polyline([(0,0),(30,0),(30,10),(10,10),(10,30),(0,30)]).close().extrude(6)\n",
        {"pen", "line", "close", "extrude"},
    ),
    "holes_on_face": (
        "import cadquery as cq\n"
        "result = (cq.Workplane('XY').rect(60, 40).extrude(10).faces('>Z').workplane()"
        ".pushPoints([(-15, 0), (15, 0)]).hole(6))\n",
        {"hole"},
    ),
    "cut": (
        "import cadquery as cq\n"
        "base = cq.Workplane('XY').rect(50, 30).extrude(10)\n"
        "pocket = cq.Workplane('XY').rect(20, 10).extrude(5).translate((0, 0, 5))\n"
        "result = base.cut(pocket)\n",
        {"plane", "extrude"},
    ),
    "union": (
        "import cadquery as cq\n"
        "base = cq.Workplane('XY').rect(50, 30).extrude(10)\n"
        "boss = cq.Workplane('XY').circle(8).extrude(20)\n"
        "result = base.union(boss)\n",
        {"extrude"},
    ),
    "fillet_chamfer": (
        "import cadquery as cq\n"
        "result = cq.Workplane('XY').rect(50, 30).extrude(10).edges('|Z').fillet(4).faces('>Z').edges().chamfer(1)\n",
        {"fillet", "chamfer"},
    ),
    "pattern": (
        "import cadquery as cq\n"
        "result = (cq.Workplane('XY').rect(80, 60).extrude(8).faces('>Z').workplane()"
        ".rarray(20, 20, 3, 2).hole(5))\n",
        {"hole"},
    ),
    "revolve": (
        "import cadquery as cq\n"
        "result = cq.Workplane('XZ').moveTo(10, 0).lineTo(20, 0).lineTo(20, 15).lineTo(10, 10).close().revolve()\n",
        {"revolve"},
    ),
}

UNSUPPORTED = "import cadquery as cq\nresult = cq.Workplane('XY').rect(10, 10).extrude(5).faces('>Z').shell(-1).edges('%CIRCLE').fillet(1)\n"


def build(gcad_text, directory):
    gcad = Path(directory) / "t.gcad"
    gcad.write_text(gcad_text)
    stl = Path(directory) / "t.stl"
    r = subprocess.run(["gcad", "export", str(gcad), str(stl)], capture_output=True, text=True, timeout=120)
    if r.returncode != 0:
        raise AssertionError(r.stdout + r.stderr)
    return load_mesh(stl)


def source_mesh(source, directory):
    namespace = {}
    exec(compile(source, "p", "exec"), namespace)
    stl = Path(directory) / "src.stl"
    real_cq.exporters.export(namespace["result"], str(stl), tolerance=0.01, angularTolerance=0.05)
    return load_mesh(stl)


class TranslatorTests(unittest.TestCase):
    def check(self, name):
        source, expected = PROGRAMS[name]
        result = translate(source)
        with tempfile.TemporaryDirectory() as tmp:
            built = build(result["gcad"], tmp)
            reference = source_mesh(source, tmp)
        metrics = compare_meshes(reference, built, volume_a=result["volume"])
        self.assertTrue(verdict(metrics), metrics)
        self.assertLessEqual(expected, set(ops_of(result["gcad"])))

    def test_rect_extrude(self):
        self.check("rect_extrude")

    def test_circle_extrude(self):
        self.check("circle_extrude")

    def test_polygon(self):
        self.check("polygon")

    def test_polyline(self):
        self.check("polyline")

    def test_holes_on_face(self):
        self.check("holes_on_face")

    def test_cut(self):
        self.check("cut")

    def test_union(self):
        self.check("union")

    def test_fillet_chamfer(self):
        self.check("fillet_chamfer")

    def test_pattern(self):
        self.check("pattern")

    def test_revolve(self):
        self.check("revolve")

    def test_unsupported_call_is_reported(self):
        with self.assertRaises(Unsupported):
            translate(UNSUPPORTED)

    def test_shape_distance_rejects_different_shape(self):
        a, _ = PROGRAMS["rect_extrude"]
        b, _ = PROGRAMS["circle_extrude"]
        with tempfile.TemporaryDirectory() as tmp:
            first = source_mesh(a, tmp)
            second = source_mesh(b, tmp)
        self.assertFalse(verdict(compare_meshes(first, second)))


if __name__ == "__main__":
    unittest.main()
