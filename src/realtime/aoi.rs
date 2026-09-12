//! Simple grid-distance AOI filter for 100-player matches.

use uuid::Uuid;

use super::match_sim::Entity;

pub const AOI_RADIUS: f32 = 28.0;

pub fn visible_entities<'a>(
    entities: impl IntoIterator<Item = &'a Entity>,
    focus_x: f32,
    focus_y: f32,
    viewer: Uuid,
) -> Vec<&'a Entity> {
    entities
        .into_iter()
        .filter(|entity| {
            if entity.owner == viewer {
                return true;
            }
            let dx = entity.x - focus_x;
            let dy = entity.y - focus_y;
            (dx * dx + dy * dy).sqrt() <= AOI_RADIUS
        })
        .collect()
}
