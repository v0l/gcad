$fn = 64;
r = 3; bend = 8;
module straight(l) cylinder(r = r, h = l);
module elbow() rotate_extrude(angle = 90) translate([bend, 0]) circle(r);
straight(12);
translate([bend, 0, 12]) multmatrix([[-1, 0, 0, 0], [0, 0, 1, 0], [0, 1, 0, 0]]) elbow();
translate([bend, 0, 20]) rotate([0, 90, 0]) straight(14);
translate([22, bend, 20]) rotate([0, 0, -90]) elbow();
translate([30, bend, 20]) rotate([-90, 0, 0]) straight(12);
