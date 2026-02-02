use bevy::prelude::*;

use crate::scene::{PrimitiveShape, SpawnEntityEvent, SpawnEntityKind};

/// Event to create a circular pattern of primitives
#[derive(Message)]
pub struct CircularPatternEvent {
    pub shape: PrimitiveShape,
    pub center: Vec3,
    pub radius: f32,
    pub count: usize,
    /// Axis to rotate around (default Y-up)
    pub axis: Vec3,
}

impl Default for CircularPatternEvent {
    fn default() -> Self {
        Self {
            shape: PrimitiveShape::Cube,
            center: Vec3::ZERO,
            radius: 5.0,
            count: 8,
            axis: Vec3::Y,
        }
    }
}

pub struct CircularPatternPlugin;

impl Plugin for CircularPatternPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CircularPatternEvent>()
            .add_systems(Update, handle_circular_pattern);
    }
}

fn handle_circular_pattern(
    mut events: MessageReader<CircularPatternEvent>,
    mut spawn_events: MessageWriter<SpawnEntityEvent>,
) {
    for event in events.read() {
        let axis = event.axis.normalize_or_zero();
        let angle_step = std::f32::consts::TAU / event.count as f32;

        // Find a perpendicular vector to the axis
        let perpendicular = if axis.dot(Vec3::X).abs() < 0.9 {
            axis.cross(Vec3::X).normalize()
        } else {
            axis.cross(Vec3::Z).normalize()
        };

        for i in 0..event.count {
            let angle = angle_step * i as f32;
            let rotation = Quat::from_axis_angle(axis, angle);
            let offset = rotation * (perpendicular * event.radius);
            let position = event.center + offset;

            spawn_events.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Primitive(event.shape),
                position,
                rotation: Quat::IDENTITY,
            });
        }

        info!(
            "Created circular pattern: {} {} at radius {}",
            event.count,
            event.shape.display_name(),
            event.radius
        );
    }
}
