$fn = 32;
r = 10; pitch = 5; turns = 4; wire = 1; steps = 72 * turns;
function at(i) = let(a = 360 * turns * i / steps) [r * cos(a), r * sin(a), pitch * turns * i / steps];
for (i = [0 : steps - 1]) hull() {
  translate(at(i)) sphere(wire);
  translate(at(i + 1)) sphere(wire);
}
