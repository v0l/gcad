use crate::Model;

#[derive(Clone, Debug)]
pub struct Measured {
    pub name: String,
    pub parts: Vec<(String, [f32; 3])>,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    pub volume: f64,
    pub glb_volume: f64,
}

impl Measured {
    pub fn size(&self) -> f64 {
        (0..3)
            .map(|k| (self.hi[k] - self.lo[k]).powi(2))
            .sum::<f64>()
            .sqrt()
    }
}

#[derive(Clone, Debug)]
pub struct Difference {
    pub body: String,
    pub size: f64,
    pub bbox: f64,
    pub kernel_volume: f64,
    pub kernel_glb_volume: f64,
    pub mesh_volume: f64,
    pub colour: f64,
}

impl Difference {
    pub fn volume(&self) -> f64 {
        (self.mesh_volume - self.kernel_volume).abs()
            / self.kernel_volume.abs().max(f64::MIN_POSITIVE)
    }

    pub fn bbox_share(&self) -> f64 {
        self.bbox / self.size.max(f64::MIN_POSITIVE)
    }

    pub fn within(&self, bound: f64) -> bool {
        self.volume() <= bound && self.bbox_share() <= bound && self.colour <= 1.0e-3
    }
}

pub fn volume_of(body: &gcad_mesh::Body) -> f64 {
    body.indices
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| body.positions[i as usize].map(f64::from));
            a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                + a[2] * (b[0] * c[1] - b[1] * c[0])
        })
        .sum::<f64>()
        / 6.0
}

pub fn measured(model: &gcad_mesh::Model) -> Vec<Measured> {
    model
        .bodies
        .iter()
        .map(|body| {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for p in &body.positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k] as f64);
                    hi[k] = hi[k].max(p[k] as f64);
                }
            }
            Measured {
                name: body.name.clone(),
                parts: body
                    .parts
                    .iter()
                    .map(|p| (p.name.clone(), p.colour))
                    .collect(),
                lo,
                hi,
                volume: volume_of(body),
                glb_volume: volume_of(body),
            }
        })
        .collect()
}

pub fn kernel(model: &Model) -> Vec<Measured> {
    let solids: Vec<(
        String,
        monstertruck::modeling::BoundingBox<monstertruck::modeling::Point3>,
        f64,
    )> = model
        .named_parts()
        .into_iter()
        .map(|(name, solid, _)| {
            let bounds = crate::geometry::bounds(solid);
            let fine = crate::geometry::volume_at(solid, bounds.diameter() * 2.0e-5);
            (name, bounds, fine)
        })
        .collect();
    let mut bodies = measured(&crate::glb::mesh_model(model, false));
    for body in &mut bodies {
        if let Some((_, bounds, fine)) = solids.iter().find(|(n, ..)| *n == body.name) {
            let (lo, hi) = (bounds.min(), bounds.max());
            body.lo = [lo.x, lo.y, lo.z];
            body.hi = [hi.x, hi.y, hi.z];
            body.volume = *fine;
        }
    }
    bodies
}

fn colour_gap(kernel: &Measured, mesh: &Measured) -> f64 {
    let Some((_, fallback)) = kernel.parts.first() else {
        return 0.0;
    };
    mesh.parts
        .iter()
        .map(|(name, colour)| {
            let theirs = kernel
                .parts
                .iter()
                .find(|(n, _)| n == name)
                .map_or(*fallback, |(_, c)| *c);
            (0..3)
                .map(|k| (colour[k] - theirs[k]).abs() as f64)
                .fold(0.0, f64::max)
        })
        .fold(0.0, f64::max)
}

pub fn compare(
    kernel: &[Measured],
    mesh: &[Measured],
) -> std::result::Result<Vec<Difference>, String> {
    let names = |bodies: &[Measured]| -> Vec<String> {
        let mut names: Vec<String> = bodies.iter().map(|b| b.name.clone()).collect();
        names.sort();
        names
    };
    if names(kernel) != names(mesh) {
        return Err(format!(
            "the kernel builds bodies {:?}, gcad-mesh {:?}",
            names(kernel),
            names(mesh)
        ));
    }
    Ok(mesh
        .iter()
        .filter_map(|m| kernel.iter().find(|k| k.name == m.name).map(|k| (k, m)))
        .map(|(k, m)| Difference {
            body: k.name.clone(),
            size: k.size(),
            bbox: (0..3)
                .map(|i| (k.lo[i] - m.lo[i]).abs().max((k.hi[i] - m.hi[i]).abs()))
                .fold(0.0, f64::max),
            kernel_volume: k.volume,
            kernel_glb_volume: k.glb_volume,
            mesh_volume: m.volume,
            colour: colour_gap(k, m),
        })
        .collect())
}
