use bevy::prelude::*;

use crate::scene::{PrimitiveShape, SpawnPrimitiveEvent};

/// Event to create a linear pattern of primitives
#[derive(Message)]
pub struct LinearPatternEvent {
    pub shape: PrimitiveShape,
    pub start: Vec3,
    pub direction: Vec3,
    pub spacing: f32,
    pub count: usize,
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
    mut spawn_events: MessageWriter<SpawnPrimitiveEvent>,
) {
    for event in events.read() {
        let direction = event.direction.normalize_or_zero();

        for i in 0..event.count {
            let position = event.start + direction * (event.spacing * i as f32);
            spawn_events.write(SpawnPrimitiveEvent {
                shape: event.shape,
                position,
                rotation: Quat::IDENTITY,
            });
        }

        info!(
            "Created linear pattern: {} {} along {:?}",
            event.count,
            event.shape.display_name(),
            direction
        );
    }
}
