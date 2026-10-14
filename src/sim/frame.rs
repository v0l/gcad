use super::skin::{self, Patch};
use super::statics::FieldView;
use super::{Field, Outcome, Reading, Spec};
use anyhow::{Result, anyhow, bail};
use monstertruck::modeling::*;
use sims_frame::{Case, FIXED, Material, Member, Model, PINNED, Section, Static};

const MM: f64 = 1.0e-3;
const JOIN: f64 = 1.0;
const SLENDER: f64 = 3.0;
const SEGMENTS: usize = 4;

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: V3, k: f64) -> V3 {
    a.map(|v| v * k)
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: V3) -> V3 {
    scale(a, 1.0 / norm(a))
}

fn point(p: Point3) -> V3 {
    [p.x, p.y, p.z]
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Combination {
    pub name: String,
    pub terms: Vec<(String, f64)>,
}

fn jacobi(mut a: [[f64; 3]; 3]) -> ([f64; 3], [V3; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..60 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        let diag = a[0][0].abs() + a[1][1].abs() + a[2][2].abs();
        if off <= 1.0e-15 * diag.max(f64::MIN_POSITIVE) {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1.0e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in a.iter_mut() {
                let (kp, kq) = (row[p], row[q]);
                row[p] = c * kp - s * kq;
                row[q] = s * kp + c * kq;
            }
            let (rp, rq) = (a[p], a[q]);
            a[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
            a[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
            for row in v.iter_mut() {
                let (kp, kq) = (row[p], row[q]);
                row[p] = c * kp - s * kq;
                row[q] = s * kp + c * kq;
            }
        }
    }
    (
        [a[0][0], a[1][1], a[2][2]],
        [
            [v[0][0], v[1][0], v[2][0]],
            [v[0][1], v[1][1], v[2][1]],
            [v[0][2], v[1][2], v[2][2]],
        ],
    )
}

#[derive(Clone)]
struct Bar {
    name: String,
    start: V3,
    end: V3,
    axis: V3,
    up: V3,
    centre: V3,
    low: f64,
    high: f64,
    area: f64,
    iy: f64,
    iz: f64,
    j: f64,
    reach: f64,
    cy: f64,
    cz: f64,
    material: Material,
    strength: Option<f64>,
}

impl Bar {
    fn length(&self) -> f64 {
        self.high - self.low
    }
}

struct Props {
    bars: Vec<Bar>,
}

fn bar_of(
    name: &str,
    patches: &[&Patch],
    material: Material,
    strength: Option<f64>,
) -> Result<Bar> {
    let (mut volume, mut moment, mut second) = (0.0, [0.0; 3], [[0.0; 3]; 3]);
    let mut vertices: Vec<V3> = Vec::new();
    for p in patches {
        vertices.extend(p.points.iter().map(|q| point(*q)));
        for t in &p.triangles {
            let [a, b, c] = t.map(|v| point(p.points[v]));
            let v = dot(a, cross(b, c)) / 6.0;
            let s = add(add(a, b), c);
            volume += v;
            for i in 0..3 {
                moment[i] += v * s[i] / 4.0;
                for k in 0..3 {
                    second[i][k] +=
                        v / 20.0 * (a[i] * a[k] + b[i] * b[k] + c[i] * c[k] + s[i] * s[k]);
                }
            }
        }
    }
    let flip = if volume < 0.0 { -1.0 } else { 1.0 };
    let volume = volume * flip;
    if volume < 1.0e-9 {
        bail!("`{name}` has no volume");
    }
    let centre = scale(moment, flip / volume);
    let mut about = [[0.0; 3]; 3];
    for i in 0..3 {
        for k in 0..3 {
            about[i][k] = second[i][k] * flip - volume * centre[i] * centre[k];
        }
    }
    let (values, vectors) = jacobi(about);
    let long = (0..3)
        .max_by(|a, b| values[*a].total_cmp(&values[*b]))
        .expect("three axes");
    let mut axis = unit(vectors[long]);
    let big = (0..3)
        .max_by(|a, b| axis[*a].abs().total_cmp(&axis[*b].abs()))
        .expect("three axes");
    if axis[big] < 0.0 {
        axis = scale(axis, -1.0);
    }
    let crosses: Vec<usize> = (0..3).filter(|k| *k != long).collect();
    let reference = if axis[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let pick = *crosses
        .iter()
        .max_by(|a, b| {
            dot(vectors[**a], reference)
                .abs()
                .total_cmp(&dot(vectors[**b], reference).abs())
        })
        .expect("two axes");
    let other = *crosses.iter().find(|k| **k != pick).expect("two axes");
    let mut up = unit(vectors[pick]);
    if dot(up, reference) < 0.0 {
        up = scale(up, -1.0);
    }
    let side = unit(cross(axis, up));
    let along = |v: V3| dot(sub(v, centre), axis);
    let (low, high) = vertices
        .iter()
        .map(|v| along(*v))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), t| {
            (l.min(t), h.max(t))
        });
    let length = high - low;
    let reach_of = |direction: V3| {
        vertices
            .iter()
            .map(|v| dot(sub(*v, centre), direction))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), t| {
                (l.min(t), h.max(t))
            })
    };
    let (up_low, up_high) = reach_of(up);
    let (side_low, side_high) = reach_of(side);
    let (depth, width) = (up_high - up_low, side_high - side_low);
    if length < SLENDER * depth.max(width) {
        bail!(
            "`{name}` is {} mm long and {} mm deep, too stout for a beam member (it needs a length of at least {SLENDER} times its depth); `leave {name}` to drop it",
            crate::plot::label(length),
            crate::plot::label(depth.max(width))
        );
    }
    let area = volume / length;
    let iz = values[pick] / length;
    let iy = values[other] / length;
    let (cy, cz) = (
        up_high.abs().max(up_low.abs()),
        side_high.abs().max(side_low.abs()),
    );
    let j = if (area - depth * width).abs() <= 0.02 * area {
        Section::rectangle(width * MM, depth * MM).j
    } else {
        (iy + iz) * 1.0e-12
    };
    Ok(Bar {
        name: name.to_string(),
        start: add(centre, scale(axis, low)),
        end: add(centre, scale(axis, high)),
        axis,
        up,
        centre,
        low,
        high,
        area: area * 1.0e-6,
        iy: iy * 1.0e-12,
        iz: iz * 1.0e-12,
        j,
        reach: (cy * cy + cz * cz).sqrt(),
        cy: cy * MM,
        cz: cz * MM,
        material,
        strength,
    })
}

fn props(spec: &Spec, patches: &[Patch]) -> Result<Props> {
    let model = spec.target()?;
    let mut bars = Vec::new();
    for (name, _) in spec.named()? {
        let Some(material) = model.materials.get(&name) else {
            bail!("give `{name}` a material, like `material steel` or `material concrete`");
        };
        let Some(young) = material.mechanics.young else {
            bail!("`{name}` has no stiffness; give it `E=` in GPa");
        };
        let nu = material.mechanics.poisson.unwrap_or(0.3);
        let own: Vec<&Patch> = patches.iter().filter(|p| p.body == name).collect();
        let found = Material::isotropic(young * 1.0e9, nu, material.density * 1000.0);
        bars.push(bar_of(&name, &own, found, material.mechanics.strength)?);
    }
    if bars.is_empty() {
        bail!("the frame has no members");
    }
    Ok(Props { bars })
}

struct Joins {
    parent: Vec<usize>,
}

impl Joins {
    fn root(&mut self, mut a: usize) -> usize {
        while self.parent[a] != a {
            self.parent[a] = self.parent[self.parent[a]];
            a = self.parent[a];
        }
        a
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        if a != b {
            self.parent[a.max(b)] = a.min(b);
        }
    }
}

struct Element {
    bar: usize,
    a: usize,
    b: usize,
}

struct Layout {
    nodes: Vec<V3>,
    elements: Vec<Element>,
    ends: Vec<[usize; 2]>,
    joined: usize,
    split: usize,
}

fn layout(bars: &[Bar], longest: Option<f64>) -> Result<Layout> {
    let n = bars.len();
    let socket = |bar: usize, end: usize| 2 * bar + end;
    let position = |s: usize| {
        if s.is_multiple_of(2) {
            bars[s / 2].start
        } else {
            bars[s / 2].end
        }
    };
    let mut interior: Vec<(usize, f64, V3)> = Vec::new();
    let mut joins = Joins {
        parent: (0..2 * n).collect(),
    };
    let mut links: Vec<(usize, usize)> = Vec::new();
    let mut joined = 0;
    for s in 0..2 * n {
        let p = position(s);
        let own = s / 2;
        let mut best: Option<(f64, usize, f64, V3)> = None;
        for (k, other) in bars.iter().enumerate() {
            if k == own {
                continue;
            }
            let t = dot(sub(p, other.centre), other.axis);
            if t < other.low - JOIN || t > other.high + JOIN {
                continue;
            }
            let q = add(other.centre, scale(other.axis, t));
            let d = norm(sub(p, q));
            if d > other.reach + JOIN {
                continue;
            }
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, k, t, q));
            }
        }
        let Some((_, k, t, q)) = best else { continue };
        joined += 1;
        if t - bars[k].low < JOIN {
            joins.union(s, socket(k, 0));
        } else if bars[k].high - t < JOIN {
            joins.union(s, socket(k, 1));
        } else {
            let at = interior
                .iter()
                .position(|(b, u, _)| *b == k && (u - t).abs() < JOIN)
                .unwrap_or_else(|| {
                    interior.push((k, t, q));
                    joins.parent.push(2 * n + interior.len() - 1);
                    interior.len() - 1
                });
            links.push((s, 2 * n + at));
        }
    }
    for (a, b) in links {
        joins.union(a, b);
    }
    let total = 2 * n + interior.len();
    let mut index = vec![usize::MAX; total];
    let mut nodes: Vec<V3> = Vec::new();
    let mut count: Vec<f64> = Vec::new();
    let mut pinned: Vec<bool> = Vec::new();
    for id in 0..total {
        let root = joins.root(id);
        if index[root] == usize::MAX {
            index[root] = nodes.len();
            nodes.push([0.0; 3]);
            count.push(0.0);
            pinned.push(false);
        }
        let node = index[root];
        let here = if id < 2 * n {
            position(id)
        } else {
            interior[id - 2 * n].2
        };
        if id >= 2 * n {
            nodes[node] = here;
            count[node] = f64::INFINITY;
            pinned[node] = true;
        } else if !pinned[node] {
            nodes[node] = add(nodes[node], here);
            count[node] += 1.0;
        }
    }
    for (node, c) in count.iter().enumerate() {
        if c.is_finite() {
            nodes[node] = scale(nodes[node], 1.0 / c);
        }
    }
    let group = |id: usize, joins: &mut Joins| index[joins.root(id)];
    let mut ends = Vec::new();
    let mut elements = Vec::new();
    for (i, bar) in bars.iter().enumerate() {
        let (a, b) = (
            group(socket(i, 0), &mut joins),
            group(socket(i, 1), &mut joins),
        );
        ends.push([a, b]);
        let mut stops: Vec<(f64, usize)> = vec![(bar.low, a), (bar.high, b)];
        for (m, (owner, _, _)) in interior.iter().enumerate() {
            if *owner == i {
                let node = group(2 * n + m, &mut joins);
                stops.push((dot(sub(nodes[node], bar.centre), bar.axis), node));
            }
        }
        stops.sort_by(|x, y| x.0.total_cmp(&y.0));
        stops.dedup_by_key(|s| s.1);
        if stops.len() < 2 {
            bail!("`{}` has both ends on one joint", bar.name);
        }
        for pair in stops.windows(2) {
            let (from, to) = (nodes[pair[0].1], nodes[pair[1].1]);
            let span = norm(sub(to, from));
            let pieces = match longest {
                Some(size) => (span / size).ceil().max(1.0) as usize,
                None => SEGMENTS,
            };
            let mut previous = pair[0].1;
            for step in 1..pieces {
                let at = add(from, scale(sub(to, from), step as f64 / pieces as f64));
                nodes.push(at);
                elements.push(Element {
                    bar: i,
                    a: previous,
                    b: nodes.len() - 1,
                });
                previous = nodes.len() - 1;
            }
            elements.push(Element {
                bar: i,
                a: previous,
                b: pair[1].1,
            });
        }
    }
    Ok(Layout {
        nodes,
        elements,
        ends,
        joined,
        split: interior.len(),
    })
}

#[derive(Clone, Copy)]
enum Where {
    End(usize),
    Side,
}

struct Faces {
    found: Vec<(usize, usize, Where, f64, V3)>,
}

fn faces(bars: &[Bar], patches: &[Patch]) -> Faces {
    let mut found = Vec::new();
    for p in patches {
        let Some(bar) = bars.iter().position(|b| b.name == p.body) else {
            continue;
        };
        let b = &bars[bar];
        let (mut area, mut normal, mut middle) = (0.0, [0.0; 3], [0.0; 3]);
        for t in &p.triangles {
            let [x, y, z] = t.map(|v| point(p.points[v]));
            let n = cross(sub(y, x), sub(z, x));
            let a = norm(n) / 2.0;
            area += a;
            normal = add(normal, scale(n, 0.5));
            middle = add(middle, scale(add(add(x, y), z), a / 3.0));
        }
        if area <= 0.0 {
            continue;
        }
        let middle = scale(middle, 1.0 / area);
        let along = dot(sub(middle, b.centre), b.axis);
        let flat = norm(normal) > 0.999 * area;
        let facing = dot(unit(normal), b.axis);
        let tolerance = JOIN + 0.01 * b.length();
        let place = if flat && facing.abs() > 0.999 && (along - b.low).abs() < tolerance {
            Where::End(0)
        } else if flat && facing.abs() > 0.999 && (along - b.high).abs() < tolerance {
            Where::End(1)
        } else {
            Where::Side
        };
        found.push((p.tag, bar, place, area, unit(normal)));
    }
    Faces { found }
}

struct Built {
    model: Model,
    layout: Layout,
    cases: Vec<(String, Case)>,
    holds: Vec<(String, Vec<usize>)>,
    combos: Vec<Combination>,
    props: Props,
    patches: Vec<Patch>,
}

fn node_of(layout: &Layout, bar: usize, end: usize) -> usize {
    layout.ends[bar][end]
}

fn setup(spec: &Spec) -> Result<Built> {
    let patches = skin::patches(spec, 0.0)?;
    let props = props(spec, &patches)?;
    let layout = layout(&props.bars, spec.mesh)?;
    let found = faces(&props.bars, &patches);
    let lookup = |tag: usize| found.found.iter().find(|f| f.0 == tag);
    let mut members: Vec<Member> = layout
        .elements
        .iter()
        .map(|e| {
            let bar = &props.bars[e.bar];
            Member {
                a: e.a,
                b: e.b,
                section: Section {
                    area: bar.area,
                    iy: bar.iy,
                    iz: bar.iz,
                    j: bar.j,
                },
                material: bar.material,
                up: bar.up,
                pinned: [false; 2],
            }
        })
        .collect();
    for faces in &spec.releases {
        for tag in faces {
            let Some(&(_, bar, place, _, _)) = lookup(*tag) else {
                bail!("`release` names a face the frame does not have");
            };
            let Where::End(end) = place else {
                bail!(
                    "`release` takes the end faces of members; a side face of `{}` is not one",
                    props.bars[bar].name
                );
            };
            let mine: Vec<usize> = (0..layout.elements.len())
                .filter(|k| layout.elements[*k].bar == bar)
                .collect();
            let element = if end == 0 { mine.first() } else { mine.last() };
            if let Some(&k) = element {
                members[k].pinned[end] = true;
            }
        }
    }
    let mut supports: Vec<(usize, [bool; 6])> = Vec::new();
    let mut holds = Vec::new();
    for support in &spec.supports {
        let mut nodes = Vec::new();
        for tag in &support.faces {
            let Some(&(_, bar, place, _, normal)) = lookup(*tag) else {
                bail!("`{}` names a face the frame does not have", support.label);
            };
            let Where::End(end) = place else {
                bail!(
                    "`{}` holds a side face of `{}`; hold the end faces of members",
                    support.label,
                    props.bars[bar].name
                );
            };
            let node = node_of(&layout, bar, end);
            let dofs = match support.hold {
                super::fem::elastic::Hold::Fixed => FIXED,
                super::fem::elastic::Hold::Pin { .. } => {
                    let mut held = PINNED;
                    let bar = &props.bars[bar];
                    if let Some(k) = (0..3).find(|k| bar.axis[*k].abs() > 0.999) {
                        held[3 + k] = true;
                    }
                    held
                }
                super::fem::elastic::Hold::Slide => {
                    let axis = (0..3).find(|k| normal[*k].abs() > 0.999).ok_or_else(|| {
                        anyhow!(
                            "`{}` slides on an end of `{}` that is not square to X, Y or Z",
                            support.label,
                            props.bars[bar].name
                        )
                    })?;
                    let mut held = [false; 6];
                    held[axis] = true;
                    held
                }
                super::fem::elastic::Hold::Bearing { .. } => bail!(
                    "`bearing` is for round shafts; hold a frame with `fix`, `pin` or `slide`"
                ),
            };
            supports.push((node, dofs));
            nodes.push(node);
        }
        match holds
            .iter_mut()
            .find(|h: &&mut (String, Vec<usize>)| h.0 == support.label)
        {
            Some(existing) => existing.1.extend(nodes),
            None => holds.push((support.label.clone(), nodes)),
        }
    }
    for hold in &mut holds {
        hold.1.sort_unstable();
        hold.1.dedup();
    }
    if supports.is_empty() {
        bail!("hold the frame with `fix`, `pin` or `slide` on the end faces of some members");
    }
    let mut cases: Vec<(String, Case)> = Vec::new();
    if spec.gravity.is_some() {
        cases.push((
            "weight".into(),
            Case {
                gravity: Some(spec.gravity().into()),
                ..Default::default()
            },
        ));
    }
    for load in &spec.loads {
        let super::fem::elastic::Push::Force(force) = load.push else {
            bail!(
                "a frame takes `load faces force=x,y,z`; `{}` is a pressure or a torque",
                load.label
            );
        };
        let force = point(Point3::from_vec(force));
        let shares: Vec<(usize, usize, Where, f64)> = load
            .faces
            .iter()
            .map(|tag| {
                lookup(*tag)
                    .map(|f| (f.0, f.1, f.2, f.3))
                    .ok_or_else(|| anyhow!("`{}` names a face the frame does not have", load.label))
            })
            .collect::<Result<_>>()?;
        let total: f64 = shares.iter().map(|s| s.3).sum();
        let index = match cases.iter().position(|c| c.0 == load.label) {
            Some(i) => i,
            None => {
                cases.push((load.label.clone(), Case::default()));
                cases.len() - 1
            }
        };
        for (_, bar, place, area) in shares {
            let part = scale(force, area / total);
            match place {
                Where::End(end) => {
                    let node = node_of(&layout, bar, end);
                    cases[index]
                        .1
                        .nodal
                        .push((node, [part[0], part[1], part[2], 0.0, 0.0, 0.0]));
                }
                Where::Side => {
                    let mine: Vec<usize> = (0..layout.elements.len())
                        .filter(|k| layout.elements[*k].bar == bar)
                        .collect();
                    let length: f64 = mine
                        .iter()
                        .map(|k| {
                            let e = &layout.elements[*k];
                            norm(sub(layout.nodes[e.b], layout.nodes[e.a])) * MM
                        })
                        .sum();
                    for k in mine {
                        cases[index].1.uniform.push((k, scale(part, 1.0 / length)));
                    }
                }
            }
        }
    }
    if cases.is_empty() && spec.modes.is_none() {
        bail!("load the frame with `load faces force=x,y,z`, or `gravity` for its own weight");
    }
    if cases.is_empty() && spec.buckling.is_some() {
        bail!("`buckling` needs loads to scale; add `load faces force=x,y,z`");
    }
    let mut combos = spec.combinations.clone();
    for combo in &combos {
        for (case, _) in &combo.terms {
            if !cases.iter().any(|c| &c.0 == case) {
                bail!(
                    "`combine {}` names no load `{case}`; loads are {}",
                    combo.name,
                    cases
                        .iter()
                        .map(|c| c.0.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    }
    if combos.is_empty() && !cases.is_empty() {
        combos.push(Combination {
            name: String::new(),
            terms: cases.iter().map(|c| (c.0.clone(), 1.0)).collect(),
        });
    }
    let nodes = layout.nodes.iter().map(|n| scale(*n, MM)).collect();
    Ok(Built {
        model: Model {
            nodes,
            members,
            supports,
            masses: Vec::new(),
        },
        layout,
        cases,
        holds,
        combos,
        props,
        patches,
    })
}

pub fn plan(spec: &Spec) -> Result<String> {
    let b = setup(spec)?;
    let mut lines = vec![format!(
        "{} members become {} elements on {} nodes; {} ends joined, {} members split by a joint",
        b.props.bars.len(),
        b.layout.elements.len(),
        b.layout.nodes.len(),
        b.layout.joined,
        b.layout.split
    )];
    for bar in &b.props.bars {
        lines.push(format!(
            "{}: {} mm, area {} mm2, I {} and {} mm4, along {:.2},{:.2},{:.2}",
            bar.name,
            crate::plot::label(bar.length()),
            crate::plot::label(bar.area * 1.0e6),
            crate::plot::label(bar.iz * 1.0e12),
            crate::plot::label(bar.iy * 1.0e12),
            bar.axis[0],
            bar.axis[1],
            bar.axis[2]
        ));
    }
    Ok(lines.join("\n"))
}

fn reading(name: String, value: f64, unit: &str, at: String) -> Reading {
    Reading {
        name,
        value,
        unit: unit.into(),
        at,
    }
}

fn stresses(b: &Built, result: &Static) -> Vec<[f64; 2]> {
    b.layout
        .elements
        .iter()
        .enumerate()
        .map(|(k, e)| {
            let bar = &b.props.bars[e.bar];
            let f = &result.ends[k];
            let at = |o: usize| {
                let axial = f[o].abs() / bar.area;
                let bend = f[o + 5].abs() * bar.cy / bar.iz + f[o + 4].abs() * bar.cz / bar.iy;
                (axial + bend) / 1.0e6
            };
            [at(0), at(6)]
        })
        .collect()
}

struct Worst {
    stress: f64,
    element: usize,
    displacement: f64,
    node: usize,
}

fn worst(b: &Built, result: &Static) -> Worst {
    let stress = stresses(b, result);
    let (element, top) = stress
        .iter()
        .enumerate()
        .flat_map(|(k, s)| [(k, s[0]), (k, s[1])])
        .fold((0, 0.0), |m, (k, v)| if v > m.1 { (k, v) } else { m });
    let (node, displacement) = result
        .displacement
        .iter()
        .enumerate()
        .map(|(n, d)| (n, (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * 1.0e3))
        .fold((0, 0.0), |m, (n, v)| if v > m.1 { (n, v) } else { m });
    Worst {
        stress: top,
        element,
        displacement,
        node,
    }
}

fn where_is(b: &Built, node: usize) -> String {
    let p = b.layout.nodes[node];
    format!("at {:.0},{:.0},{:.0}", p[0], p[1], p[2])
}

fn nearest(b: &Built, p: V3) -> (usize, f64) {
    let mut best = (0, f64::INFINITY, 0.0);
    for (k, e) in b.layout.elements.iter().enumerate() {
        let (x, y) = (b.layout.nodes[e.a], b.layout.nodes[e.b]);
        let d = sub(y, x);
        let t = (dot(sub(p, x), d) / dot(d, d)).clamp(0.0, 1.0);
        let q = add(x, scale(d, t));
        let distance = norm(sub(p, q));
        if distance < best.1 {
            best = (k, distance, t);
        }
    }
    (best.0, best.2)
}

pub fn run(spec: &Spec) -> Result<Outcome> {
    let b = setup(spec)?;
    let mut solved: Vec<(String, Static)> = Vec::new();
    let stage = crate::progress::stage("solving the load cases", Some(b.cases.len()), "cases");
    let statics: Vec<Static> = b
        .cases
        .iter()
        .map(|(_, case)| {
            let s = sims_frame::solve(&b.model, case).map_err(|e| anyhow!(e));
            stage.advance(1.0);
            s
        })
        .collect::<Result<_>>()?;
    for combo in &b.combos {
        let parts: Vec<(&Static, f64)> = combo
            .terms
            .iter()
            .map(|(case, factor)| {
                let at = b.cases.iter().position(|c| &c.0 == case).expect("checked");
                (&statics[at], *factor)
            })
            .collect();
        let combined =
            Static::combine(&parts).ok_or_else(|| anyhow!("`combine {}` is empty", combo.name))?;
        solved.push((combo.name.clone(), combined));
    }
    let mut readings = Vec::new();
    let mut governing: Option<(usize, f64)> = None;
    for (n, (name, result)) in solved.iter().enumerate() {
        let prefix = if name.is_empty() {
            String::new()
        } else {
            format!("{name}.")
        };
        let w = worst(&b, result);
        readings.push(reading(
            format!("{prefix}displacement"),
            w.displacement,
            "mm",
            where_is(&b, w.node),
        ));
        let e = &b.layout.elements[w.element];
        readings.push(reading(
            format!("{prefix}stress"),
            w.stress,
            "MPa",
            format!("in `{}` {}", b.props.bars[e.bar].name, where_is(&b, e.a)),
        ));
        if governing.is_none_or(|g| w.stress > g.1) {
            governing = Some((n, w.stress));
        }
        let stress = stresses(&b, result);
        let mut safety: Option<(f64, usize)> = None;
        for (k, s) in stress.iter().enumerate() {
            let bar = &b.props.bars[b.layout.elements[k].bar];
            let Some(limit) = bar.strength else { continue };
            let top = s[0].max(s[1]);
            if top > 0.0 && safety.is_none_or(|m| limit / top < m.0) {
                safety = Some((limit / top, k));
            }
        }
        if let Some((value, k)) = safety {
            let bar = &b.props.bars[b.layout.elements[k].bar];
            readings.push(reading(
                format!("{prefix}safety"),
                value,
                "",
                format!("yield over stress in `{}`, the weakest member", bar.name),
            ));
        }
        for (label, nodes) in &b.holds {
            let mut force = [0.0; 3];
            for node in nodes {
                for (total, part) in force.iter_mut().zip(&result.reaction[*node]) {
                    *total += part;
                }
            }
            readings.push(reading(
                format!("{prefix}{label}"),
                norm(force),
                "N",
                format!(
                    "reaction x {:.0}, y {:.0}, z {:.0} N",
                    force[0], force[1], force[2]
                ),
            ));
        }
        for (probe, p) in &spec.probes {
            let (k, t) = nearest(&b, point(*p));
            let e = &b.layout.elements[k];
            let (da, db) = (result.displacement[e.a], result.displacement[e.b]);
            let d: Vec<f64> = (0..3).map(|i| da[i] + (db[i] - da[i]) * t).collect();
            readings.push(reading(
                format!("{prefix}{probe}.displacement"),
                norm([d[0], d[1], d[2]]) * 1.0e3,
                "mm",
                format!("in `{}`", b.props.bars[e.bar].name),
            ));
            let s = stresses(&b, result)[k];
            readings.push(reading(
                format!("{prefix}{probe}.stress"),
                s[0] + (s[1] - s[0]) * t,
                "MPa",
                format!("in `{}`", b.props.bars[e.bar].name),
            ));
        }
    }
    if let Some(count) = spec.modes {
        let found = sims_frame::modes(&b.model, count).map_err(|e| anyhow!(e))?;
        for (k, mode) in found.iter().enumerate() {
            readings.push(reading(
                format!("mode{}", k + 1),
                mode.frequency(),
                "Hz",
                "free vibration of the frame's own mass".into(),
            ));
        }
    }
    if let Some(count) = spec.buckling {
        let mut all = Case::default();
        for (_, case) in &b.cases {
            all.nodal.extend(case.nodal.iter().cloned());
            all.uniform.extend(case.uniform.iter().cloned());
            all.gravity = all.gravity.or(case.gravity);
        }
        let found = sims_frame::buckling(&b.model, &all, count).map_err(|e| anyhow!(e))?;
        for (k, mode) in found.iter().enumerate() {
            readings.push(reading(
                format!("buckling{}", k + 1),
                mode.value,
                "",
                "load factor: the loads all together times this buckle the frame".into(),
            ));
        }
    }
    let mut notes = vec![format!(
        "{} members, {} elements, {} nodes; {} ends joined to another member, {} members split by a joint",
        b.props.bars.len(),
        b.layout.elements.len(),
        b.layout.nodes.len(),
        b.layout.joined,
        b.layout.split
    )];
    notes.push(
        "each body is a straight prismatic member: its axis, section area and bending stiffness come from the solid, its ends move onto the axis of the member they meet, and stress is beam theory (axial plus bending at the outer fibre), blind to joints, shear and local buckling"
            .into(),
    );
    let field = governing.map(|(n, _)| field_of(&b, &solved[n].1, &solved[n].0));
    Ok(Outcome {
        kind: "frame".into(),
        readings,
        notes,
        field,
        ..Default::default()
    })
}

fn along_chain(b: &Built, bar: usize, p: V3) -> (usize, usize, f64) {
    let mut best = (0, 0, 0.0, f64::INFINITY);
    for (k, e) in b.layout.elements.iter().enumerate() {
        if e.bar != bar {
            continue;
        }
        let (x, y) = (b.layout.nodes[e.a], b.layout.nodes[e.b]);
        let d = sub(y, x);
        let t = (dot(sub(p, x), d) / dot(d, d)).clamp(0.0, 1.0);
        let q = add(x, scale(d, t));
        let distance = norm(sub(p, q));
        if distance < best.3 {
            best = (k, e.a, t, distance);
        }
    }
    (best.0, best.1, best.2)
}

fn field_of(b: &Built, result: &Static, name: &str) -> Field {
    let stress = stresses(b, result);
    let mut values = Vec::new();
    let mut displacement = Vec::new();
    for patch in &b.patches {
        let Some(bar) = b.props.bars.iter().position(|x| x.name == patch.body) else {
            continue;
        };
        for q in &patch.points {
            let p = point(*q);
            let (k, _, t) = along_chain(b, bar, p);
            let e = &b.layout.elements[k];
            let s = stress[k];
            values.push(s[0] + (s[1] - s[0]) * t);
            let (da, db) = (result.displacement[e.a], result.displacement[e.b]);
            let u: Vec<f64> = (0..6).map(|i| da[i] + (db[i] - da[i]) * t).collect();
            let (xa, xb) = (b.layout.nodes[e.a], b.layout.nodes[e.b]);
            let on = add(xa, scale(sub(xb, xa), t));
            let turn = cross([u[3], u[4], u[5]], sub(p, on));
            displacement.push([
                u[0] * 1.0e3 + turn[0],
                u[1] * 1.0e3 + turn[1],
                u[2] * 1.0e3 + turn[2],
            ]);
        }
    }
    Field {
        title: if name.is_empty() {
            "stress".into()
        } else {
            format!("stress, {name}")
        },
        unit: "MPa".into(),
        values,
        displacement,
    }
}

pub fn field_view(spec: &Spec, outcome: Option<&Outcome>) -> Result<FieldView> {
    let patches = skin::patches(spec, 0.0)?;
    let total: usize = patches.iter().map(|p| p.points.len()).sum();
    let (values, moved, title, unit) = match outcome.and_then(|o| o.field.as_ref()) {
        Some(f) if f.values.len() == total && f.displacement.len() == total => (
            f.values.clone(),
            f.displacement
                .iter()
                .map(|d| Vector3::new(d[0], d[1], d[2]))
                .collect(),
            f.title.clone(),
            f.unit.clone(),
        ),
        Some(_) => bail!("the result does not match the frame; rerun `gcad sim`"),
        None => {
            let mut v = Vec::new();
            let mut body = 0;
            let mut seen: Vec<String> = Vec::new();
            for p in &patches {
                if !seen.contains(&p.body) {
                    seen.push(p.body.clone());
                    body = seen.len();
                }
                v.extend(std::iter::repeat_n(body as f64, p.points.len()));
            }
            (
                v,
                vec![Vector3::zero(); total],
                "the frame".to_string(),
                String::new(),
            )
        }
    };
    let mut rest: Vec<Point3> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut edges: Vec<[u32; 3]> = Vec::new();
    for patch in &patches {
        let base = rest.len();
        rest.extend(patch.points.iter().copied());
        triangles.extend(patch.triangles.iter().map(|t| t.map(|v| (base + v) as u32)));
        edges.extend(
            super::wind::creases(&patch.points, &patch.triangles)
                .into_iter()
                .map(|[a, b]| [a, a, b].map(|v| (base + v) as u32)),
        );
    }
    let bounds: BoundingBox<Point3> = rest.iter().copied().collect();
    let largest = moved.iter().map(|d| d.magnitude()).fold(0.0, f64::max);
    let scale_by = if largest > 0.0 {
        bounds.diameter() * 0.05 / largest
    } else {
        0.0
    };
    let positions: Vec<Point3> = rest
        .iter()
        .zip(&moved)
        .map(|(p, d)| p + d * scale_by)
        .collect();
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = if unit == "MPa" {
        0.0
    } else {
        values.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let span = (high - low).max(1.0e-12);
    Ok(FieldView {
        shade: values
            .iter()
            .map(|v| ((v - low) / span).clamp(0.0, 1.0))
            .collect(),
        triangles,
        edges,
        low,
        high: low + span,
        scale: scale_by,
        title,
        unit,
        moved,
        rest,
        values,
        positions,
        marks: Vec::new(),
        index: Default::default(),
        slices: Vec::new(),
        streams: Vec::new(),
        travel: Default::default(),
        movie: None,
    })
}
