//! Uniform spatial hash: O(k) neighbor queries instead of scanning every entity.
//! Cell size ~1 world unit so weapon range / vision discs touch a small neighborhood.

use std::collections::HashMap;

use uuid::Uuid;

/// World units per cell. Matches typical rifle range (4.5) and infantry radius.
pub const CELL_SIZE: f32 = 1.0;

/// Generous upper bound on building/unit collision radius (HQ ≈ 0.90).
pub const MAX_ENTITY_RADIUS: f32 = 1.05;

/// Largest unit footprint (tank). Used for soft-separation neighbor queries.
pub const MAX_UNIT_RADIUS: f32 = 0.12;

pub struct SpatialGrid {
    cell: f32,
    cells: HashMap<u64, Vec<Uuid>>,
    loc: HashMap<Uuid, (i32, i32)>,
}

impl Default for SpatialGrid {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialGrid {
    pub fn new() -> Self {
        Self {
            cell: CELL_SIZE,
            cells: HashMap::with_capacity(4096),
            loc: HashMap::with_capacity(8192),
        }
    }

    fn cell_xy(&self, x: f32, y: f32) -> (i32, i32) {
        (
            (x / self.cell).floor() as i32,
            (y / self.cell).floor() as i32,
        )
    }

    fn pack(ix: i32, iy: i32) -> u64 {
        ((ix as u32 as u64) << 32) | (iy as u32 as u64)
    }

    pub fn clear(&mut self) {
        self.cells.clear();
        self.loc.clear();
    }

    pub fn rebuild(&mut self, entities: impl Iterator<Item = (Uuid, f32, f32)>) {
        self.clear();
        for (id, x, y) in entities {
            self.upsert(id, x, y);
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        let Some((ix, iy)) = self.loc.remove(&id) else {
            return;
        };
        let key = Self::pack(ix, iy);
        let Some(bucket) = self.cells.get_mut(&key) else {
            return;
        };
        if let Some(i) = bucket.iter().position(|&item| item == id) {
            bucket.swap_remove(i);
        }
        if bucket.is_empty() {
            self.cells.remove(&key);
        }
    }

    /// Insert or move `id` to the cell covering `(x, y)`.
    pub fn upsert(&mut self, id: Uuid, x: f32, y: f32) {
        let (ix, iy) = self.cell_xy(x, y);
        if let Some(&(ox, oy)) = self.loc.get(&id) {
            if ox == ix && oy == iy {
                return;
            }
            self.remove(id);
        }
        self.loc.insert(id, (ix, iy));
        self.cells.entry(Self::pack(ix, iy)).or_default().push(id);
    }

    /// Visit ids in cells overlapping the disc. `visit` returning true stops the walk.
    pub fn for_each_nearby(
        &self,
        x: f32,
        y: f32,
        radius: f32,
        visit: impl FnMut(Uuid) -> bool,
    ) -> bool {
        if radius <= 0.0 {
            return false;
        }
        self.for_each_in_aabb(x - radius, y - radius, x + radius, y + radius, visit)
    }

    pub fn for_each_in_aabb(
        &self,
        min_x: f32,
        min_y: f32,
        max_x: f32,
        max_y: f32,
        mut visit: impl FnMut(Uuid) -> bool,
    ) -> bool {
        let min_ix = (min_x.min(max_x) / self.cell).floor() as i32;
        let max_ix = (min_x.max(max_x) / self.cell).floor() as i32;
        let min_iy = (min_y.min(max_y) / self.cell).floor() as i32;
        let max_iy = (min_y.max(max_y) / self.cell).floor() as i32;
        for iy in min_iy..=max_iy {
            for ix in min_ix..=max_ix {
                let Some(bucket) = self.cells.get(&Self::pack(ix, iy)) else {
                    continue;
                };
                for &id in bucket {
                    if visit(id) {
                        return true;
                    }
                }
            }
        }
        false
    }
}
