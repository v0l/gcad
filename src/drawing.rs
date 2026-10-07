use crate::export::Colour;
use crate::geometry;
use monstertruck::modeling::*;
use std::collections::HashMap;
use std::fmt::Write as _;

struct View {
    name: &'static str,
    right: Vector3,
    up: Vector3,
}

impl View {
    fn toward(&self) -> Vector3 {
        self.right.cross(self.up)
    }

    fn project(&self, p: Point3) -> (f64, f64) {
        (p.to_vec().dot(self.right), p.to_vec().dot(self.up))
    }
}

fn polylines(solid: &Solid) -> Vec<Vec<Point3>> {
    solid
        .boundaries()
        .iter()
        .flat_map(|shell| shell.edge_iter().collect::<Vec<_>>())
        .fold(Vec::<Edge>::new(), |mut unique, edge| {
            if !unique.iter().any(|e| e.is_same(&edge)) {
                unique.push(edge);
            }
            unique
        })
        .iter()
        .map(|edge| {
            let curve = edge.curve();
            let (t0, t1) = curve.range_tuple();
            let straight = matches!(curve, Curve::Line(_));
            let steps = if straight { 1 } else { 32 };
            (0..=steps)
                .map(|i| curve.subs(t0 + (t1 - t0) * i as f64 / steps as f64))
                .collect()
        })
        .collect()
}

fn silhouettes(solid: &Solid, toward: Vector3) -> Vec<Vec<Point3>> {
    let mesh = geometry::mesh(solid, geometry::mesh_tolerance(solid));
    let positions = mesh.positions();
    let scale = geometry::bounds(solid).diameter().max(1.0) * 1.0e-9;
    let key = |p: Point3| [p.x, p.y, p.z].map(|c| (c / scale).round() as i64);
    type Side = (Point3, Point3, Vec<f64>);
    let mut sides: HashMap<([i64; 3], [i64; 3]), Side> = HashMap::new();
    for triangle in mesh.faces().triangle_iter() {
        let [a, b, c] = triangle.map(|v| positions[v.pos]);
        let normal = (b - a).cross(c - a);
        if normal.magnitude() < 1.0e-14 {
            continue;
        }
        let facing = normal.normalize().dot(toward);
        for (p, q) in [(a, b), (b, c), (c, a)] {
            let (kp, kq) = (key(p), key(q));
            let id = if kp < kq { (kp, kq) } else { (kq, kp) };
            sides.entry(id).or_insert((p, q, Vec::new())).2.push(facing);
        }
    }
    sides
        .into_values()
        .filter(|(_, _, facing)| {
            facing.len() == 2
                && facing[0] * facing[1] < 0.0
                && facing.iter().all(|f| f.abs() > 1.0e-6)
        })
        .map(|(p, q, _)| vec![p, q])
        .collect()
}

pub fn drawing(parts: &[(&Solid, Option<Colour>)]) -> String {
    let views = [
        View {
            name: "top",
            right: Vector3::unit_x(),
            up: Vector3::unit_y(),
        },
        View {
            name: "front",
            right: Vector3::unit_x(),
            up: Vector3::unit_z(),
        },
        View {
            name: "right",
            right: Vector3::unit_y(),
            up: Vector3::unit_z(),
        },
        View {
            name: "iso",
            right: Vector3::new(1.0, 1.0, 0.0).normalize(),
            up: Vector3::new(-1.0, 1.0, 2.0).normalize(),
        },
    ];
    let bounds: BoundingBox<Point3> = parts
        .iter()
        .flat_map(|(solid, _)| {
            let b = geometry::bounds(solid);
            [b.min(), b.max()]
        })
        .collect();
    let size = bounds.max() - bounds.min();
    let gap = size.x.max(size.y).max(size.z).max(1.0) * 0.25;
    let extent = |view: &View| {
        let corners: Vec<(f64, f64)> = (0..8)
            .map(|i| {
                let pick = |bit: usize, lo: f64, hi: f64| if i & bit == 0 { lo } else { hi };
                view.project(Point3::new(
                    pick(1, bounds.min().x, bounds.max().x),
                    pick(2, bounds.min().y, bounds.max().y),
                    pick(4, bounds.min().z, bounds.max().z),
                ))
            })
            .collect();
        let fold = |f: fn(f64, f64) -> f64, start: f64, pick: fn(&(f64, f64)) -> f64| {
            corners.iter().map(pick).fold(start, f)
        };
        (
            fold(f64::min, f64::INFINITY, |c| c.0),
            fold(f64::max, f64::NEG_INFINITY, |c| c.0),
            fold(f64::min, f64::INFINITY, |c| c.1),
            fold(f64::max, f64::NEG_INFINITY, |c| c.1),
        )
    };
    let extents: Vec<(f64, f64, f64, f64)> = views.iter().map(extent).collect();
    let width = |i: usize| extents[i].1 - extents[i].0;
    let height = |i: usize| extents[i].3 - extents[i].2;
    let column = [width(0).max(width(1)), width(2).max(width(3))];
    let row = [height(0).max(height(3)), height(1).max(height(2))];
    let cells = [(0, 0), (0, 1), (1, 1), (1, 0)];
    let total_w = gap * 3.0 + column[0] + column[1];
    let total_h = gap * 4.0 + row[0] + row[1];
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total_w:.3}mm\" height=\"{total_h:.3}mm\" viewBox=\"0 0 {total_w:.3} {total_h:.3}\">"
    );
    let stroke = gap * 0.01;
    let _ = writeln!(svg, "<rect width=\"100%\" height=\"100%\" fill=\"white\"/>");
    for (i, view) in views.iter().enumerate() {
        let (col, rowi) = cells[i];
        let left = gap + if col == 1 { column[0] + gap } else { 0.0 };
        let top = gap + if rowi == 1 { row[0] + gap } else { 0.0 };
        let (x0, _, _, y1) = extents[i];
        let place = |p: Point3| {
            let (u, v) = view.project(p);
            (left + (u - x0), top + (y1 - v))
        };
        let _ = writeln!(
            svg,
            "<g id=\"{}\" fill=\"none\" stroke=\"black\" stroke-width=\"{stroke:.4}\" stroke-linecap=\"round\">",
            view.name
        );
        for (solid, _) in parts {
            let lines = polylines(solid)
                .into_iter()
                .chain(silhouettes(solid, view.toward()));
            for line in lines {
                let mut d = String::new();
                for (k, p) in line.iter().enumerate() {
                    let (x, y) = place(*p);
                    let _ = write!(d, "{}{x:.3},{y:.3} ", if k == 0 { "M" } else { "L" });
                }
                let _ = writeln!(svg, "<path d=\"{}\"/>", d.trim_end());
            }
        }
        svg.push_str("</g>\n");
        let _ = writeln!(
            svg,
            "<text x=\"{left:.3}\" y=\"{:.3}\" font-family=\"sans-serif\" font-size=\"{:.3}\">{}</text>",
            top - gap * 0.2,
            gap * 0.18,
            view.name
        );
    }
    let _ = writeln!(
        svg,
        "<text x=\"{:.3}\" y=\"{:.3}\" font-family=\"sans-serif\" font-size=\"{:.3}\">{:.2} x {:.2} x {:.2} mm</text>",
        gap,
        total_h - gap * 0.4,
        gap * 0.18,
        size.x,
        size.y,
        size.z
    );
    svg.push_str("</svg>\n");
    svg
}
