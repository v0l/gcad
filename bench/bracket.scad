$fn = 64;
w = 60; d = 40; t = 6;
module outline(w, h, r) offset(r) square([w - 2 * r, h - 2 * r], center = true);
module tapered(w, h, r, depth, angle) {
  inset = depth * tan(angle);
  hull() {
    linear_extrude(1e-3) outline(w, h, r);
    translate([0, 0, depth - 1e-3]) linear_extrude(1e-3) outline(w - 2 * inset, h - 2 * inset, r - inset);
  }
}
difference() {
  union() {
    hull() {
      linear_extrude(t - 1) outline(w, d, 5);
      translate([0, 0, t - 1]) minkowski() {
        linear_extrude(1e-3) outline(w - 2, d - 2, 4);
        sphere(1);
      }
    }
    translate([18, 8, t]) difference() {
      cylinder(d1 = 12, d2 = 12 - 2 * 10 * tan(2), h = 10);
      translate([0, 0, 10 - 0.5]) difference() {
        cylinder(d = 20, h = 1);
        cylinder(d1 = 12 - 2 * 10 * tan(2) - 1, d2 = 12 - 2 * 10 * tan(2) + 1, h = 1);
      }
    }
  }
  translate([-8, 0, t]) mirror([0, 0, 1]) tapered(30, 16, 3, 3, 4);
  translate([18, 8, t + 10 - 8]) cylinder(d = 4, h = 9);
}
