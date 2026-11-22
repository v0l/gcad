use monstertruck::modeling::*;

const SIZE: usize = 2048;

pub struct ShadowMap {
    toward_sun: Vector3,
    right: Vector3,
    up: Vector3,
    origin: (f64, f64),
    scale: f64,
    depth: Vec<f64>,
}

impl ShadowMap {
    pub fn new(
        positions: &[Point3],
        triangles: &[[usize; 3]],
        toward_sun: Vector3,
    ) -> ShadowMap {
        let toward_sun = toward_sun.normalize();
        let seed = if toward_sun.z.abs() > 0.99 {
            Vector3::unit_y()
        } else {
            Vector3::unit_z()
        };
        let right = seed.cross(toward_sun).normalize();
        let up = toward_sun.cross(right);
        let flat: Vec<(f64, f64, f64)> = positions
            .iter()
            .map(|p| {
                let v = p.to_vec();
                (v.dot(right), v.dot(up), v.dot(toward_sun))
            })
            .collect();
        let lo = flat.iter().fold((f64::INFINITY, f64::INFINITY), |a, p| (a.0.min(p.0), a.1.min(p.1)));
        let hi = flat
            .iter()
            .fold((f64::NEG_INFINITY, f64::NEG_INFINITY), |a, p| (a.0.max(p.0), a.1.max(p.1)));
        let span = (hi.0 - lo.0).max(hi.1 - lo.1).max(1.0e-9);
        let scale = (SIZE as f64 - 2.0) / span;
        let mut map = ShadowMap {
            toward_sun,
            right,
            up,
            origin: lo,
            scale,
            depth: vec![f64::NEG_INFINITY; SIZE * SIZE],
        };
        for t in triangles {
            let p = t.map(|i| {
                let q = flat[i];
                ((q.0 - lo.0) * scale + 1.0, (q.1 - lo.1) * scale + 1.0, q.2)
            });
            map.fill(p);
        }
        map
    }

    fn fill(&mut self, p: [(f64, f64, f64); 3]) {
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() < 1.0e-12 {
            return;
        }
        let bound = |f: fn(&(f64, f64, f64)) -> f64, lo: bool| {
            let v = p.iter().map(f);
            if lo {
                v.fold(f64::INFINITY, f64::min).floor().max(0.0) as i64
            } else {
                v.fold(f64::NEG_INFINITY, f64::max).ceil().min(SIZE as f64 - 1.0) as i64
            }
        };
        let (x0, x1) = (bound(|q| q.0, true), bound(|q| q.0, false));
        let (y0, y1) = (bound(|q| q.1, true), bound(|q| q.1, false));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let w0 = ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
                let w1 = ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
                let w2 = 1.0 - w0 - w1;
                let e = -1.0e-6;
                if w0 < e || w1 < e || w2 < e {
                    continue;
                }
                let z = w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2;
                let slot = &mut self.depth[y as usize * SIZE + x as usize];
                if z > *slot {
                    *slot = z;
                }
            }
        }
    }

    pub fn lit(&self, point: Point3) -> bool {
        let v = point.to_vec();
        let x = ((v.dot(self.right) - self.origin.0) * self.scale + 1.0).floor();
        let y = ((v.dot(self.up) - self.origin.1) * self.scale + 1.0).floor();
        if x < 0.0 || y < 0.0 || x >= SIZE as f64 || y >= SIZE as f64 {
            return true;
        }
        let nearest = self.depth[y as usize * SIZE + x as usize];
        v.dot(self.toward_sun) >= nearest - 3.0 / self.scale.max(1.0e-9) - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_shades_the_ground_behind_it() {
        let p = |x: f64, y: f64, z: f64| Point3::new(x, y, z);
        let positions = vec![
            p(-5000.0, -5000.0, 0.0),
            p(5000.0, -5000.0, 0.0),
            p(5000.0, 5000.0, 0.0),
            p(-5000.0, 5000.0, 0.0),
            p(-1000.0, 0.0, 0.0),
            p(1000.0, 0.0, 0.0),
            p(1000.0, 0.0, 3000.0),
            p(-1000.0, 0.0, 3000.0),
        ];
        let triangles = [[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]];
        let sun = Vector3::new(0.0, -1.0, 1.0);
        let map = ShadowMap::new(&positions, &triangles, sun);
        assert!(!map.lit(p(0.0, 1500.0, 0.0)));
        assert!(map.lit(p(0.0, -1500.0, 0.0)));
        assert!(map.lit(p(3000.0, 1500.0, 0.0)));
        assert!(map.lit(p(0.0, 4000.0, 0.0)));
    }
}
