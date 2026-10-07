$fn = 64;
w = 40; h = 30; t = 3; r = 4; f = 0.8; hole = 3.2; c = 0.3;
module outline(w, h, r) offset(r) square([w - 2 * r, h - 2 * r], center = true);
difference() {
  hull() {
    linear_extrude(t - f) outline(w, h, r);
    translate([0, 0, t - f]) minkowski() {
      linear_extrude(1e-3) outline(w - 2 * f, h - 2 * f, r - f);
      sphere(f);
    }
  }
  for (x = [-15, 15], y = [-10, 10]) translate([x, y, 0]) {
    translate([0, 0, -1]) cylinder(d = hole, h = t + 2);
    translate([0, 0, t - c]) cylinder(d1 = hole, d2 = hole + 2 * c + 0.02, h = c + 0.01);
  }
}
