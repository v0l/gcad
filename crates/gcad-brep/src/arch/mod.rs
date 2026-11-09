mod datum;
mod opening;
mod roof;
mod room;
mod slab;
mod stair;
mod structure;
mod wall;

use crate::geometry::Profile;
use gcad_mesh::arch::{Chain, Join, Justify, Layer};

#[derive(Clone, Debug, Default)]
pub struct Building {
    pub name: String,
    pub north: f64,
    pub datum: f64,
}

#[derive(Clone, Debug)]
pub struct Level {
    pub name: String,
    pub z: f64,
    pub title: String,
}

#[derive(Clone, Debug)]
pub struct Grid {
    pub name: String,
    pub along_x: bool,
    pub offset: f64,
}

#[derive(Clone, Debug)]
pub struct Tier {
    pub layers: Vec<Layer>,
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Debug)]
pub struct Wall {
    pub name: String,
    pub line: Chain,
    pub tiers: Vec<Tier>,
    pub justify: Justify,
    pub offset: f64,
    pub base: f64,
    pub top: f64,
    pub top_offset: (f64, f64),
    pub batter: f64,
    pub pilasters: Vec<(f64, f64, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Head {
    Flat,
    Round,
    Arch,
    Segment,
}

#[derive(Clone, Debug)]
pub enum Fill {
    Hole,
    Cut(Vec<Profile>),
    Door {
        swing: String,
        inward: bool,
        frame: (f64, f64),
        leaf: f64,
        threshold: f64,
    },
    Window {
        panes: (usize, usize),
        frame: f64,
        depth: f64,
        mullion: f64,
        glass: f64,
    },
    Garage {
        panels: usize,
    },
    Part {
        file: std::path::PathBuf,
        vars: Vec<(String, f64)>,
    },
}

#[derive(Clone, Debug)]
pub struct Opening {
    pub name: String,
    pub host: String,
    pub at: f64,
    pub w: f64,
    pub h: f64,
    pub sill: f64,
    pub head: Head,
    pub mark: Option<String>,
    pub fill: Fill,
}

#[derive(Clone, Debug)]
pub struct Sweep {
    pub name: String,
    pub host: String,
    pub face: String,
    pub width: f64,
    pub height: f64,
    pub at: f64,
    pub material: String,
}

#[derive(Clone, Debug)]
pub struct Slab {
    pub name: String,
    pub outline: Vec<gcad_mesh::arch::P2>,
    pub holes: Vec<Vec<gcad_mesh::arch::P2>>,
    pub layers: Vec<Layer>,
    pub top: f64,
    pub slope: f64,
    pub fall: Option<(gcad_mesh::arch::P2, gcad_mesh::arch::P2)>,
}

#[derive(Clone, Debug)]
pub struct Roof {
    pub name: String,
    pub kind: String,
    pub outline: Vec<gcad_mesh::arch::P2>,
    pub overhang: f64,
    pub pitch: f64,
    pub base: f64,
    pub ridge: gcad_mesh::arch::P2,
    pub thickness: f64,
    pub material: String,
}

#[derive(Clone, Debug)]
pub struct Room {
    pub name: String,
    pub level: f64,
    pub rect: [f64; 4],
}

#[derive(Clone, Debug)]
pub struct Flight {
    pub start: gcad_mesh::arch::P2,
    pub dir: gcad_mesh::arch::P2,
    pub z0: f64,
    pub treads: usize,
}

#[derive(Clone, Debug)]
pub struct Piece {
    pub kind: &'static str,
    pub ring: Vec<gcad_mesh::arch::P2>,
    pub top: f64,
    pub depth: f64,
}

#[derive(Clone, Debug)]
pub struct Stair {
    pub name: String,
    pub base: f64,
    pub width: f64,
    pub steps: gcad_mesh::arch::Steps,
    pub flights: Vec<Flight>,
    pub pieces: Vec<Piece>,
}

#[derive(Clone, Debug)]
pub struct Built {
    pub owner: String,
    pub key: String,
    pub holes: Vec<String>,
    pub bodies: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Arch {
    pub units: Option<String>,
    pub building: Option<Building>,
    pub levels: Vec<Level>,
    pub grids: Vec<Grid>,
    pub types: Vec<(String, Vec<Layer>)>,
    pub walls: Vec<Wall>,
    pub joins: Vec<(String, String, Join)>,
    pub openings: Vec<Opening>,
    pub sweeps: Vec<Sweep>,
    pub slabs: Vec<Slab>,
    pub roofs: Vec<Roof>,
    pub rooms: Vec<Room>,
    pub stairs: Vec<Stair>,
    pub built: Vec<Built>,
}

impl Arch {
    pub fn owns(&self, body: &str) -> bool {
        self.built
            .iter()
            .any(|b| b.bodies.iter().any(|name| name == body))
    }

    pub fn wall(&self, name: &str) -> Option<&Wall> {
        self.walls.iter().find(|w| w.name == name)
    }
}
