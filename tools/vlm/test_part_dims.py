import random
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import pairs_data
import part_dims

PLATE = "rect 100 60 at=0,0\nextrude 5\nfillet 2 +X,-X&+Y,-Y\nplane 0,0,5 1,0,5 0,1,5\nhole 6 -40,-20 40,-20 40,20 -40,20\n"
DISC = "circle 50 at=0,0\nextrude 8\nchamfer 1 >Z&all\nhole 5 0,0 depth=6\n"
POCKET = "rect 80 50 at=0,0\nextrude 20\nplane 0,0,20 1,0,20 0,1,20\nrect 60 30 at=0,0\nextrude 12 mode=cut\n"
BOSS = "rect 80 30 at=40,15\nextrude 6\nplane XY\ncircle 12 at=0,0\nextrude 10\n"
POLY = "poly 0,0 80,0 80,20 20,20 20,80 0,80\nextrude 5\n"
NGON = "ngon 40 6 at=0,0 angle=0\nextrude 10\n"
SLOT = "rect 60 30 at=0,0\nextrude 5\nslot 40 10 at=0,0 angle=0\nextrude 5 mode=cut\n"
REVOLVE = "plane 0,0,0 1,0,0 0,0,1\npen 10,0\nline 10,3\nline 30,5\nline 30,50\nline 8,50\nline 8,60\nline 10,60\nclose\nrevolve 360 axis=y\n"
SHELL = "rect 40 30 at=0,0\nextrude 20\nshell 2 open=s1.end\n"
LOFT = "rect 20 15 at=0,0\nsection\nplane 0,0,80 1,0,80 0,1,80\nrect 12 10 at=0,0\nsection\nloft smooth\n"
GRID = "rect 120 80 at=0,0\nextrude 6\nhole 4 -20,-10 0,-10 20,-10 -20,10 0,10 20,10\n"
CBORE = "rect 50 50 at=0,0\nextrude 10\nhole 4 -15,0 15,0 cbore=7,3\n"

ALL = [PLATE, DISC, POCKET, BOSS, POLY, NGON, SLOT, REVOLVE, SHELL, LOFT, GRID, CBORE]


def spec_text(program):
    return part_dims.render(part_dims.extract(program), random.Random(0), p=0.0)


class Extraction(unittest.TestCase):
    def test_plate_with_corner_fillet_and_four_holes(self):
        text = spec_text(PLATE)
        self.assertIn("100 x 60", text)
        self.assertIn("5 mm", text)
        self.assertIn("four holes", text)
        self.assertIn("6 mm dia", text)
        self.assertIn("through", text)
        self.assertIn("80 x 40", text)
        self.assertIn("2 mm fillet", text)
        self.assertIn("vertical corner edges", text)

    def test_disc_with_chamfer_and_blind_hole(self):
        text = spec_text(DISC)
        self.assertIn("50 mm diameter", text)
        self.assertIn("8 mm", text)
        self.assertIn("1 mm chamfer", text)
        self.assertIn("top edges", text)
        self.assertRegex(text, r"(6 mm deep|depth 6)")

    def test_pocket_cut_gives_size_and_depth(self):
        text = spec_text(POCKET)
        self.assertIn("80 x 50", text)
        self.assertIn("pocket, 60 x 30", text)
        self.assertIn("12", text)

    def test_boss_on_a_plate(self):
        text = spec_text(BOSS)
        self.assertIn("80 x 30", text)
        self.assertIn("round boss, 12 mm diameter", text)
        self.assertRegex(text, r"(10 mm tall|extruded 10 mm)")

    def test_polyline_outline_reports_overall_size(self):
        self.assertIn("80 x 80 overall", spec_text(POLY))

    def test_ngon_reports_sides_and_corner_diameter(self):
        text = spec_text(NGON)
        self.assertIn("six-sided", text)
        self.assertIn("40 mm across corners", text)

    def test_slot_cut(self):
        text = spec_text(SLOT)
        self.assertIn("slot, 40 x 10", text)

    def test_revolve_reports_diameter_and_length(self):
        text = spec_text(REVOLVE)
        self.assertRegex(text, r"60 mm (maximum|largest) diameter")
        self.assertRegex(text, r"60 mm (long|tall)")

    def test_shell_reports_wall_thickness(self):
        self.assertIn("2 mm", spec_text(SHELL))
        self.assertIn("40 x 30", spec_text(SHELL))

    def test_loft_reports_both_sections(self):
        text = spec_text(LOFT)
        self.assertIn("20 x 15", text)
        self.assertIn("12 x 10", text)

    def test_hole_grid_pattern(self):
        text = spec_text(GRID)
        self.assertIn("six holes", text)
        self.assertIn("20 x 20", text)

    def test_counterbored_holes(self):
        text = spec_text(CBORE)
        self.assertIn("counterbored 7 mm wide, 3 mm deep", text)

    def test_numbers_are_rounded_like_a_person(self):
        text = spec_text("rect 12.7000001 25.4 at=0,0\nextrude 3.30000004\n")
        self.assertIn("12.7 x 25.4", text)
        self.assertNotIn("0000", text)

    def test_unsupported_ops_are_reported_not_guessed(self):
        spec = part_dims.extract("rect 10 10 at=0,0\nextrude 5\nthread M6 on=a.side\nwibble 3\n")
        self.assertEqual(spec.unphrased, ["thread", "wibble"])


class NoInventedNumbers(unittest.TestCase):
    def test_every_number_comes_from_the_program(self):
        for program in ALL:
            for seed in range(10):
                text = part_dims.render(part_dims.extract(program), random.Random(seed), p=0.3)
                self.assertEqual(part_dims.invented_numbers(program, text), [], msg=f"{program!r}\n{text}")

    def test_invented_number_is_detected(self):
        self.assertEqual(part_dims.invented_numbers(PLATE, "plate 100 x 61"), [61.0])


class Dropout(unittest.TestCase):
    def test_same_seed_same_spec(self):
        a = part_dims.describe(PLATE, "k", p=0.3, seed=7)
        b = part_dims.describe(PLATE, "k", p=0.3, seed=7)
        self.assertEqual(a, b)

    def test_seed_and_key_change_the_spec(self):
        texts = {part_dims.describe(PLATE, f"k{i}", p=0.3, seed=7) for i in range(20)}
        self.assertGreater(len(texts), 5)

    def test_main_sizes_are_never_dropped(self):
        for seed in range(50):
            text = part_dims.describe(PLATE, "k", p=1.0, seed=seed)
            self.assertIn("100 x 60", text)
            self.assertIn("5 mm", text)

    def test_p_one_drops_minor_dimensions(self):
        text = part_dims.describe(PLATE, "k", p=1.0, seed=1)
        self.assertNotIn("80 x 40", text)
        self.assertNotIn("6 mm", text)

    def test_p_zero_keeps_everything(self):
        text = part_dims.describe(PLATE, "k", p=0.0, seed=1)
        self.assertIn("80 x 40", text)
        self.assertIn("2 mm fillet", text)

    def test_about_p_of_minor_clauses_are_dropped(self):
        kept = sum("80 x 40" in part_dims.describe(PLATE, f"k{i}", p=0.3, seed=1) or "40 spacing" in part_dims.describe(PLATE, f"k{i}", p=0.3, seed=1) for i in range(400))
        self.assertTrue(220 < kept < 340, kept)

    def test_phrasing_varies(self):
        starts = {part_dims.describe(PLATE, f"k{i}", p=0.0, seed=1)[:12] for i in range(40)}
        self.assertGreater(len(starts), 1)


class Prompt(unittest.TestCase):
    def test_default_prompt_is_unchanged(self):
        self.assertEqual(pairs_data.prompt_for(PLATE, "k"), pairs_data.PROMPT)

    def test_dims_prompt_carries_the_spec(self):
        text = pairs_data.prompt_for(PLATE, "k", dims=True, p=0.0)
        self.assertIn("Dimensions in mm:", text)
        self.assertIn("100 x 60", text)
        self.assertTrue(text.endswith("Reply with the program only."))

    def test_program_without_a_spec_keeps_the_old_prompt(self):
        self.assertEqual(pairs_data.prompt_for("wibble 3\n", "k", dims=True), pairs_data.PROMPT)


if __name__ == "__main__":
    unittest.main()
