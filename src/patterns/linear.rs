use bevy::prelude::*;

use super::{spawn_pattern, PatternPosition};
use crate::scene::{PrimitiveShape, SpawnEntityEvent};

/// Event to create a linear pattern of primitives
#[derive(Message)]
pub struct LinearPatternEvent {
    pub shape: PrimitiveShape,
    pub start: Vec3,
    pub direction: Vec3,
    pub spacing: f32,
    pub count: usize,
}

impl LinearPatternEvent {
    /// Generate positions for this linear pattern
    fn generate_positions(&self) -> impl Iterator<Item = PatternPosition> + '_ {
        let direction = self.direction.normalize_or_zero();
        (0..self.count).map(move |i| {
            let position = self.start + direction * (self.spacing * i as f32);
            PatternPosition::new(position)
        })
    }
}

pub struct LinearPatternPlugin;

impl Plugin for LinearPatternPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<LinearPatternEvent>()
            .add_systems(Update, handle_linear_pattern);
    }
}

fn handle_linear_pattern(
    mut events: MessageReader<LinearPatternEvent>,
    mut spawn_events: MessageWriter<SpawnEntityEvent>,
) {
    for event in events.read() {
        let count = spawn_pattern(&mut spawn_events, event.shape, event.generate_positions());
        let direction = event.direction.normalize_or_zero();

        info!(
            "Created linear pattern: {} {} along {:?}",
            count,
            event.shape.display_name(),
            direction
        );
    }
}
