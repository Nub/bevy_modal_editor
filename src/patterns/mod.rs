mod circular;
mod linear;

pub use circular::*;
pub use linear::*;

use bevy::prelude::*;

use crate::scene::{PrimitiveShape, SpawnEntityEvent, SpawnEntityKind};

pub struct PatternsPlugin;

impl Plugin for PatternsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(LinearPatternPlugin)
            .add_plugins(CircularPatternPlugin);
    }
}

/// A generated pattern position with optional rotation
pub struct PatternPosition {
    pub position: Vec3,
    pub rotation: Quat,
}

impl PatternPosition {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            rotation: Quat::IDENTITY,
        }
    }

    pub fn with_rotation(position: Vec3, rotation: Quat) -> Self {
        Self { position, rotation }
    }
}

/// Spawn entities for a pattern at the given positions
pub fn spawn_pattern(
    spawn_events: &mut MessageWriter<SpawnEntityEvent>,
    shape: PrimitiveShape,
    positions: impl IntoIterator<Item = PatternPosition>,
) -> usize {
    let mut count = 0;
    for pos in positions {
        spawn_events.write(SpawnEntityEvent {
            kind: SpawnEntityKind::Primitive(shape),
            position: pos.position,
            rotation: pos.rotation,
        });
        count += 1;
    }
    count
}
