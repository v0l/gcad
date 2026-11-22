use super::{Fill, Wall};
use crate::Model;
use anyhow::{Result, anyhow, bail};
use gcad_mesh::arch::P2;
use gcad_mesh::arch::layout::signed_area;
use gcad_mesh::arch::roof::cells;
use std::collections::BTreeMap;

pub const KINDS: &[&str] = &[
    "doors", "windows", "rooms", "areas", "zones", "finishes", "walls", "slabs", "roofs",
    "stairs", "columns", "materials", "members",
];

const RATE_UNITS: [&str; 4] = ["m2", "m3", "m", "item"];

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Default)]
pub struct Rates(BTreeMap<String, BTreeMap<String, f64>>);

impl Rates {
    pub fn parse(text: &str) -> Result<Rates> {
        let table: toml::Table = text.parse()?;
        let mut rates = BTreeMap::new();
        for (material, value) in table {
            let toml::Value::Table(units) = value else {
                bail!(
                    "rates: `{material}` should be a table like `[{material}]` with m2, m3, m or item"
                );
            };
            let mut prices = BTreeMap::new();
            for (unit, price) in units {
                if !RATE_UNITS.contains(&unit.as_str()) {
                    bail!("rates: `{material}` has unit `{unit}`, use m2, m3, m or item");
                }
                let price = price
                    .as_float()
                    .or_else(|| price.as_integer().map(|i| i as f64))
                    .ok_or_else(|| anyhow!("rates: `{material}.{unit}` should be a number"))?;
                prices.insert(unit, price);
            }
            rates.insert(material, prices);
        }
        Ok(Rates(rates))
    }

    fn cost(&self, material: &str, q: &Quantity) -> f64 {
        let Some(prices) = self.0.get(material) else {
            return 0.0;
        };
        RATE_UNITS
            .iter()
            .find_map(|unit| {
                prices.get(*unit).map(|price| match *unit {
                    "m2" => price * q.area,
                    "m3" => price * q.volume,
                    "m" => price * q.length,
                    _ => price * q.count,
                })
            })
            .unwrap_or(0.0)
    }
}

pub fn number(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() {
        "0".into()
    } else {
        text.into()
    }
}

fn quoted(cell: &str) -> String {
    if cell.contains([',', '"', '\n']) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell.to_string()
    }
}

impl Table {
    pub fn csv(&self) -> String {
        let line = |cells: &[String]| {
            cells
                .iter()
                .map(|c| quoted(c))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut out = line(&self.header);
        out.push('\n');
        for row in &self.rows {
            out.push_str(&line(row));
            out.push('\n');
        }
        out
    }

    pub fn json(&self) -> String {
        let rows: Vec<serde_json::Value> = self
            .rows
            .iter()
            .map(|row| {
                let mut object = serde_json::Map::new();
                for (key, cell) in self.header.iter().zip(row) {
                    let value = cell
                        .parse::<f64>()
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map_or_else(
                            || serde_json::Value::String(cell.clone()),
                            serde_json::Value::Number,
                        );
                    object.insert(key.clone(), value);
                }
                serde_json::Value::Object(object)
            })
            .collect();
        serde_json::to_string_pretty(&rows).unwrap_or_default() + "\n"
    }

    fn sorted(mut self) -> Table {
        self.rows.sort_by(|a, b| a[0].cmp(&b[0]));
        self
    }

    fn with_cost(mut self, rates: &Rates, rows: &[(String, Quantity)]) -> Table {
        self.header.push("cost".into());
        let mut total = 0.0;
        for (row, (material, q)) in self.rows.iter_mut().zip(rows) {
            let cost = rates.cost(material, q);
            total += cost;
            row.push(number(cost));
        }
        let mut last = vec![String::new(); self.header.len()];
        last[0] = "total".into();
        last[self.header.len() - 1] = number(total);
        self.rows.push(last);
        self
    }
}

#[derive(Default)]
struct Quantity {
    area: f64,
    volume: f64,
    length: f64,
    count: f64,
}

fn table(header: &[&str], rows: Vec<Vec<String>>) -> Table {
    Table {
        header: header.iter().map(|h| h.to_string()).collect(),
        rows,
    }
}

fn polyline_length(points: &[P2], closed: bool) -> f64 {
    let n = points.len();
    let pairs = if closed { n } else { n.saturating_sub(1) };
    (0..pairs)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            (b.0 - a.0).hypot(b.1 - a.1)
        })
        .sum()
}

fn ring_at(wall: &Wall, d: f64) -> Vec<P2> {
    wall.line
        .offset(d, None, None)
        .into_iter()
        .map(|(p, _)| p)
        .collect()
}

fn centre_length(wall: &Wall) -> f64 {
    let (lo, hi) = wall.outer_span();
    polyline_length(&ring_at(wall, (lo + hi) / 2.0), wall.line.closed)
}

impl Model {
    fn level_name(&self, z: f64) -> String {
        self.arch
            .levels
            .iter()
            .find(|l| (l.z - z).abs() < 1.0e-6)
            .map(|l| l.name.clone())
            .unwrap_or_default()
    }

    fn openings_of(&self, wall: &Wall) -> Vec<&super::Opening> {
        self.arch
            .openings
            .iter()
            .filter(|o| o.host == wall.name && !matches!(o.fill, Fill::Cut(_)))
            .collect()
    }

    fn wall_layers(&self, wall: &Wall) -> Vec<(String, f64, f64)> {
        let length = centre_length(wall);
        let openings = self.openings_of(wall);
        let mut out = Vec::new();
        for tier in &wall.tiers {
            for layer in &tier.layers {
                let cut: f64 = openings
                    .iter()
                    .map(|o| {
                        let overlap = (o.sill + o.h).min(tier.to) - o.sill.max(tier.from);
                        o.w * overlap.max(0.0)
                    })
                    .sum();
                let area = (length * (tier.to - tier.from) - cut) / 1.0e6;
                out.push((
                    layer.material.clone(),
                    area,
                    area * layer.thickness / 1000.0,
                ));
            }
        }
        out
    }

    fn slab_area(&self, slab: &super::Slab) -> (f64, f64) {
        let gross = signed_area(&slab.outline).abs() / 1.0e6;
        let holes: f64 = slab
            .holes
            .iter()
            .map(|h| signed_area(h).abs())
            .sum::<f64>()
            / 1.0e6;
        (gross, gross - holes)
    }

    fn roof_area(&self, roof: &super::Roof) -> Result<f64> {
        let planes = roof.planes()?;
        Ok(cells(&roof.eaves(), &planes)
            .iter()
            .map(|(i, cell)| {
                let g = planes[*i].g;
                signed_area(cell).abs() * (1.0 + g.0 * g.0 + g.1 * g.1).sqrt()
            })
            .sum::<f64>()
            / 1.0e6)
    }

    fn column_volume(&self, name: &str) -> f64 {
        self.named_body(name)
            .map(|s| crate::geometry::volume(&s).abs() / 1.0e9)
            .unwrap_or(0.0)
    }

    fn quantities(&self) -> Result<BTreeMap<String, Quantity>> {
        let mut found: BTreeMap<String, Quantity> = BTreeMap::new();
        let mut add = |material: &str, area: f64, volume: f64, length: f64, count: f64| {
            let q = found.entry(material.to_string()).or_default();
            q.area += area;
            q.volume += volume;
            q.length += length;
            q.count += count;
        };
        for wall in &self.arch.walls {
            for (material, area, volume) in self.wall_layers(wall) {
                add(&material, area, volume, 0.0, 0.0);
            }
        }
        for slab in &self.arch.slabs {
            let (_, net) = self.slab_area(slab);
            for layer in &slab.layers {
                add(
                    &layer.material,
                    net,
                    net * layer.thickness / 1000.0,
                    0.0,
                    0.0,
                );
            }
        }
        for roof in &self.arch.roofs {
            let area = self.roof_area(roof)?;
            add(
                &roof.material,
                area,
                area * roof.thickness / 1000.0,
                0.0,
                0.0,
            );
        }
        for column in &self.arch.columns {
            add(
                &column.material,
                0.0,
                self.column_volume(&column.name),
                (column.top - column.base) / 1000.0,
                1.0,
            );
        }
        Ok(found)
    }

    fn members(&self) -> Table {
        let mut groups: BTreeMap<String, (usize, f64)> = BTreeMap::new();
        for source in self.sources.values() {
            let profile = source.file.display().to_string();
            let Some(length) = source
                .body
                .split_whitespace()
                .next()
                .and_then(|w| w.parse::<f64>().ok())
            else {
                continue;
            };
            if !["bar ", "tube ", "pipe ", "rod ", "angle ", "channel "]
                .iter()
                .any(|k| profile.starts_with(k))
            {
                continue;
            }
            let entry = groups.entry(profile).or_default();
            entry.0 += 1;
            entry.1 += length / 1000.0;
        }
        table(
            &["profile", "count", "length"],
            groups
                .into_iter()
                .map(|(p, (count, length))| vec![p, count.to_string(), number(length)])
                .collect(),
        )
    }

    pub fn schedule(&self, kind: &str, rates: Option<&Rates>) -> Result<Table> {
        let arch = &self.arch;
        let result = match kind {
            "doors" | "windows" => {
                let wanted_door = kind == "doors";
                let rows = arch
                    .openings
                    .iter()
                    .filter(|o| match o.fill {
                        Fill::Door { .. } | Fill::Garage { .. } => wanted_door,
                        Fill::Window { .. } => !wanted_door,
                        _ => false,
                    })
                    .map(|o| {
                        let base = arch.wall(&o.host).map_or(0.0, |w| w.base);
                        vec![
                            o.mark.clone().unwrap_or_else(|| o.name.clone()),
                            o.name.clone(),
                            o.host.clone(),
                            self.level_name(base),
                            number(o.w),
                            number(o.h),
                            number(o.sill),
                            number(o.w * o.h / 1.0e6),
                        ]
                    })
                    .collect();
                table(
                    &[
                        "mark", "name", "host", "level", "width", "height", "sill", "area",
                    ],
                    rows,
                )
            }
            "rooms" => table(
                &[
                    "name",
                    "level",
                    "number",
                    "title",
                    "floor",
                    "ceiling",
                    "wall",
                    "net_area",
                    "perimeter",
                    "height",
                    "volume",
                ],
                arch.rooms
                    .iter()
                    .map(|r| {
                        vec![
                            r.name.clone(),
                            self.level_name(r.level),
                            r.number.clone(),
                            r.title.clone(),
                            r.floor.clone(),
                            r.ceiling.clone(),
                            r.wall.clone(),
                            number(r.area()),
                            number(r.perimeter()),
                            number(r.height / 1000.0),
                            number(r.volume()),
                        ]
                    })
                    .collect(),
            ),
            "finishes" => table(
                &[
                    "name",
                    "level",
                    "floor",
                    "ceiling",
                    "wall",
                    "floor_area",
                    "ceiling_area",
                    "wall_area",
                ],
                arch.rooms
                    .iter()
                    .map(|r| {
                        vec![
                            r.name.clone(),
                            self.level_name(r.level),
                            r.floor.clone(),
                            r.ceiling.clone(),
                            r.wall.clone(),
                            number(r.area()),
                            number(r.area()),
                            number(r.perimeter() * r.height / 1000.0),
                        ]
                    })
                    .collect(),
            ),
            "areas" => {
                let scheme = arch.area_scheme.clone().unwrap_or_else(|| "net".into());
                let rows = arch
                    .levels
                    .iter()
                    .filter_map(|level| {
                        let gross = arch
                            .walls
                            .iter()
                            .filter(|w| {
                                w.line.closed
                                    && w.base <= level.z + 1.0e-6
                                    && level.z < w.top - 1.0e-6
                            })
                            .map(|w| {
                                let (lo, hi) = w.outer_span();
                                [lo, hi]
                                    .iter()
                                    .map(|d| signed_area(&ring_at(w, *d)).abs())
                                    .fold(0.0, f64::max)
                                    / 1.0e6
                            })
                            .fold(0.0, f64::max);
                        let net: f64 = arch
                            .rooms
                            .iter()
                            .filter(|r| (r.level - level.z).abs() < 1.0e-6)
                            .map(|r| r.area())
                            .sum();
                        (gross > 0.0 || net > 0.0).then(|| {
                            let area = if scheme == "gross" { gross } else { net };
                            vec![
                                level.name.clone(),
                                scheme.clone(),
                                number(area),
                                number(gross),
                                number(net),
                            ]
                        })
                    })
                    .collect();
                table(
                    &["level", "scheme", "area", "gross_area", "net_area"],
                    rows,
                )
            }
            "zones" => table(
                &["name", "rooms", "net_area", "volume"],
                arch.zones
                    .iter()
                    .map(|(name, members)| {
                        let rooms: Vec<_> = arch
                            .rooms
                            .iter()
                            .filter(|r| members.contains(&r.name))
                            .collect();
                        vec![
                            name.clone(),
                            members.join(" "),
                            number(rooms.iter().map(|r| r.area()).sum()),
                            number(rooms.iter().map(|r| r.volume()).sum()),
                        ]
                    })
                    .collect(),
            ),
            "walls" => table(
                &[
                    "name",
                    "level",
                    "length",
                    "height",
                    "thickness",
                    "gross_area",
                    "net_area",
                    "volume",
                    "openings",
                ],
                arch.walls
                    .iter()
                    .map(|w| {
                        let length = centre_length(w);
                        let height = w.top - w.base;
                        let openings = self.openings_of(w);
                        let cut: f64 = openings.iter().map(|o| o.w * o.h).sum();
                        let volume: f64 = self.wall_layers(w).iter().map(|l| l.2).sum();
                        vec![
                            w.name.clone(),
                            self.level_name(w.base),
                            number(length / 1000.0),
                            number(height / 1000.0),
                            number(w.thickness()),
                            number(length * height / 1.0e6),
                            number((length * height - cut) / 1.0e6),
                            number(volume),
                            openings.len().to_string(),
                        ]
                    })
                    .collect(),
            ),
            "slabs" => table(
                &["name", "top", "thickness", "gross_area", "area", "volume"],
                arch.slabs
                    .iter()
                    .map(|s| {
                        let (gross, net) = self.slab_area(s);
                        let t: f64 = s.layers.iter().map(|l| l.thickness).sum();
                        vec![
                            s.name.clone(),
                            number(s.top),
                            number(t),
                            number(gross),
                            number(net),
                            number(net * t / 1000.0),
                        ]
                    })
                    .collect(),
            ),
            "roofs" => {
                let mut rows = Vec::new();
                for r in &arch.roofs {
                    let area = self.roof_area(r)?;
                    rows.push(vec![
                        r.name.clone(),
                        r.kind.clone(),
                        number(r.pitch),
                        number(r.thickness),
                        number(area),
                        number(area * r.thickness / 1000.0),
                    ]);
                }
                table(
                    &["name", "type", "pitch", "thickness", "area", "volume"],
                    rows,
                )
            }
            "stairs" => table(
                &[
                    "name", "width", "risers", "rise", "tread", "going", "flights",
                ],
                arch.stairs
                    .iter()
                    .map(|s| {
                        vec![
                            s.name.clone(),
                            number(s.width),
                            s.steps.risers.to_string(),
                            number(s.steps.rise),
                            number(s.steps.tread),
                            number(s.steps.going() / 1000.0),
                            s.flights.len().to_string(),
                        ]
                    })
                    .collect(),
            ),
            "columns" => table(
                &["name", "section", "height", "material", "volume"],
                arch.columns
                    .iter()
                    .map(|c| {
                        vec![
                            c.name.clone(),
                            format!("{} {}", c.kind, c.size),
                            number((c.top - c.base) / 1000.0),
                            c.material.clone(),
                            number(self.column_volume(&c.name)),
                        ]
                    })
                    .collect(),
            ),
            "materials" => {
                let found: Vec<(String, Quantity)> = self.quantities()?.into_iter().collect();
                let rows = found
                    .iter()
                    .map(|(name, q)| {
                        vec![
                            name.clone(),
                            number(q.area),
                            number(q.volume),
                            number(q.length),
                            number(q.count),
                        ]
                    })
                    .collect();
                let built = table(&["name", "area", "volume", "length", "count"], rows);
                return Ok(match rates {
                    Some(rates) => built.with_cost(rates, &found),
                    None => built,
                });
            }
            "members" => self.members(),
            other => bail!("no schedule `{other}`; schedules are {}", KINDS.join(", ")),
        };
        Ok(result.sorted())
    }
}
