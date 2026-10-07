$fn = 64;
w = 60; d = 40; h = 25; wall = 2;
module outline(w, h, r) offset(r) square([w - 2 * r, h - 2 * r], center = true);
difference() {
  linear_extrude(h) outline(w, d, 5);
  translate([0, 0, wall]) linear_extrude(h) outline(w - 2 * wall, d - 2 * wall, 5 - wall);
  for (x = [-22, 22], y = [-12, 12]) translate([x, y, 0]) {
    translate([0, 0, -1]) cylinder(d = 3.2, h = wall + 2);
    translate([0, 0, -0.01]) cylinder(d1 = 6.4, d2 = 3.2, h = 1.6);
  }
  translate([-14, -3, wall - 0.6]) linear_extrude(1) text("LINECAD", size = 6);
}
