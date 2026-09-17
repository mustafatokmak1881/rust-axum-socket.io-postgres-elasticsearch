//! Generals-style vision + explored fog-of-war helpers.
//! Hot-path visibility uses the spatial grid in `MatchSim::visible_ids_for`.

use super::match_sim::Entity;

/// Soft network/client hint (HQ-scale vision).
pub const AOI_RADIUS: f32 = 20.0;

pub const VISION_HQ: f32 = 20.0;
pub const VISION_BUILDING: f32 = 13.0;
pub const VISION_UNIT: f32 = 11.0;

pub fn entity_provides_vision(entity: &Entity) -> bool {
    entity.building || entity.unit
}

pub fn vision_radius(entity: &Entity) -> f32 {
    if entity.kind == "hq" {
        VISION_HQ
    } else if entity.building {
        VISION_BUILDING
    } else if entity.unit {
        VISION_UNIT
    } else {
        0.0
    }
}

#[derive(Clone, Debug)]
pub struct ExploredMap {
    pub size: u16,
    bits: Vec<u64>,
}

impl ExploredMap {
    pub fn new(size: u16) -> Self {
        let cells = (size as usize).saturating_mul(size as usize);
        let words = cells.div_ceil(64);
        Self {
            size,
            bits: vec![0; words],
        }
    }

    fn index(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.size || y >= self.size {
            return None;
        }
        Some(y as usize * self.size as usize + x as usize)
    }

    /// Returns true if the cell was newly revealed.
    pub fn reveal_cell(&mut self, x: u16, y: u16) -> bool {
        let Some(i) = self.index(x, y) else {
            return false;
        };
        let word = i / 64;
        let bit = i % 64;
        let mask = 1u64 << bit;
        let was = self.bits[word] & mask != 0;
        self.bits[word] |= mask;
        !was
    }

    /// Reveal a disc; returns newly explored packed cell indices (y * size + x).
    pub fn reveal_circle(&mut self, cx: f32, cy: f32, radius: f32) -> Vec<u16> {
        let mut newly = Vec::new();
        if radius <= 0.0 {
            return newly;
        }
        let r = radius.ceil() as i32;
        let ix = cx.floor() as i32;
        let iy = cy.floor() as i32;
        let size = self.size as i32;
        let r2 = radius * radius;
        for dy in -r..=r {
            for dx in -r..=r {
                let x = ix + dx;
                let y = iy + dy;
                if x < 0 || y < 0 || x >= size || y >= size {
                    continue;
                }
                let fx = x as f32 + 0.5;
                let fy = y as f32 + 0.5;
                let ddx = fx - cx;
                let ddy = fy - cy;
                if ddx * ddx + ddy * ddy > r2 {
                    continue;
                }
                if self.reveal_cell(x as u16, y as u16) {
                    let idx = (y as u16).saturating_mul(self.size).saturating_add(x as u16);
                    newly.push(idx);
                }
            }
        }
        newly
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bits
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }
}
