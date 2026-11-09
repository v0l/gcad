use crate::cloud::{Cloud, Layout, Point};

pub struct Collector {
    cloud: Cloud,
    max: Option<usize>,
    total: Option<u64>,
    seen: u64,
    state: u64,
}

impl Collector {
    pub fn new(layout: Layout, max_points: Option<usize>, total: Option<u64>) -> Collector {
        let capacity = match (max_points, total) {
            (Some(max), Some(total)) => max.min(total as usize),
            (Some(max), None) => max.min(1 << 20),
            (None, Some(total)) => (total as usize).min(1 << 26),
            (None, None) => 0,
        };
        Collector {
            cloud: Cloud::with_layout(layout, capacity),
            max: max_points,
            total,
            seen: 0,
            state: 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn offer(&mut self, point: Point) {
        let index = self.seen;
        self.seen += 1;
        let Some(max) = self.max else {
            self.cloud.push(point);
            return;
        };
        match self.total {
            Some(total) if total > 0 => {
                let step = |i: u64| (i as u128 * max as u128 / total as u128) as u64;
                if index < total && step(index + 1) > step(index) {
                    self.cloud.push(point);
                }
            }
            _ if (index as usize) < max => self.cloud.push(point),
            _ => {
                let slot = self.next() % (index + 1);
                if (slot as usize) < max {
                    self.cloud.set(slot as usize, point);
                }
            }
        }
    }

    pub fn seen(&self) -> u64 {
        self.seen
    }

    pub fn finish(self) -> (Cloud, u64) {
        (self.cloud, self.seen)
    }
}

pub fn display_indices(len: usize, cap: usize) -> Vec<usize> {
    if len <= cap {
        return (0..len).collect();
    }
    (0..cap)
        .map(|k| (k as u128 * len as u128 / cap as u128) as usize)
        .collect()
}
