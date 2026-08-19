//! Orientation widget (owner ask): a live axis gizmo in the viewport corner
//! that shows which way the world is facing AND takes you there — click the +Z
//! ball to stand on +Z looking back at the scene, which is the front view.
//!
//! A UI widget rather than a rendered cube: the balls are real `Node`s, so they
//! are clickable by construction (picking into a render target is not), they
//! cost no camera, and they stay crisp at any DPI. Positions are the world axes
//! projected through the camera's rotation each frame, so it reads as one solid
//! object turning with the view.

use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;

use crate::style::{self, UiFonts};

/// Half-size of the widget, and how far the balls sit from its centre.
const RADIUS: f32 = 34.0;
const BALL: f32 = 20.0;

#[derive(Component)]
pub(crate) struct ViewGizmoRoot;

/// One axis ball: the world axis it represents, and the view it jumps to.
#[derive(Component, Clone, Copy)]
pub(crate) struct AxisBall {
    axis: Vec3,
    action: &'static str,
}

impl AxisBall {
    /// The world axis this ball stands for (probes read it to check tracking).
    pub(crate) fn axis(&self) -> Vec3 {
        self.axis
    }
}

/// Clicking an axis stands the camera ON that axis looking back at the scene —
/// the same six views `1`/`2`/`3` reach from the keyboard.
const AXES: [(&str, Vec3, &str); 6] = [
    ("X", Vec3::X, "view.right"),
    ("-X", Vec3::NEG_X, "view.left"),
    ("Y", Vec3::Y, "view.top"),
    ("-Y", Vec3::NEG_Y, "view.bottom"),
    ("Z", Vec3::Z, "view.front"),
    ("-Z", Vec3::NEG_Z, "view.back"),
];

fn axis_color(axis: Vec3) -> Color {
    // The same dusty triad the inspector's axis fields use — chrome volume, not
    // saturated primaries.
    if axis.x != 0.0 {
        Color::srgb(0.71, 0.44, 0.44)
    } else if axis.y != 0.0 {
        Color::srgb(0.55, 0.64, 0.42)
    } else {
        Color::srgb(0.45, 0.57, 0.75)
    }
}

pub(crate) fn spawn_view_gizmo(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
) {
    let inset = settings.ui.dock_right_width + style::space::M;
    let root = commands
        .spawn((
            ViewGizmoRoot,
            Node {
                position_type: PositionType::Absolute,
                right: px(inset),
                top: px(style::space::M),
                width: px(RADIUS * 2.0 + BALL),
                height: px(RADIUS * 2.0 + BALL),
                ..default()
            },
            GlobalZIndex(40),
            Visibility::Hidden,
        ))
        .id();
    for (label, axis, action) in AXES {
        let ball = commands
            .spawn((
                AxisBall { axis, action },
                Node {
                    position_type: PositionType::Absolute,
                    width: px(BALL),
                    height: px(BALL),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(BALL * 0.5)),
                    ..default()
                },
                BackgroundColor(axis_color(axis)),
                BorderColor::all(style::HAIRLINE),
                ChildOf(root),
            ))
            .observe(on_axis_press)
            .id();
        commands.spawn((
            Text::new(label.to_string()),
            style::no_wrap(),
            style::mono(&fonts, 9.0),
            TextColor(style::color::TEXT_ON_ACCENT),
            bevy::picking::Pickable::IGNORE,
            ChildOf(ball),
        ));
    }
}

fn on_axis_press(
    press: On<Pointer<Press>>,
    balls: Query<&AxisBall>,
    mut actions: MessageWriter<ActionInvoked>,
) {
    let Ok(ball) = balls.get(press.entity) else {
        return;
    };
    actions.write(ActionInvoked {
        action: ActionId::new(ball.action.to_string()),
        args: None,
        source: InvocationSource::Palette,
    });
}

/// Project each world axis through the camera's rotation and lay the balls out
/// accordingly: the widget turns with the view, and an axis pointing away from
/// you sits behind the others, dimmed and small.
pub(crate) fn sync_view_gizmo(
    state: Res<EditorState>,
    cameras: Query<(
        &Camera,
        &GlobalTransform,
        Option<&bevy::camera::RenderTarget>,
    )>,
    mut root: Query<&mut Visibility, With<ViewGizmoRoot>>,
    mut balls: Query<(&AxisBall, &mut Node, &mut BackgroundColor, &mut ZIndex)>,
) {
    for mut visibility in &mut root {
        let want = if state.active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
    }
    if !state.active {
        return;
    }
    let Some((_, camera_transform, _)) = cameras
        .iter()
        .find(|(camera, _, target)| is_viewport_camera(camera, target.as_deref()))
    else {
        return;
    };
    let rotation = camera_transform.rotation().inverse();
    for (ball, mut node, mut background, mut z) in &mut balls {
        // Axis in CAMERA space: x right, y up, z toward the viewer.
        let view = rotation * ball.axis;
        let centre = RADIUS;
        node.left = px(centre + view.x * RADIUS);
        node.top = px(centre - view.y * RADIUS);
        // z > 0 points at the viewer: draw those in front, at full strength.
        let facing = view.z;
        let toward = (facing + 1.0) * 0.5; // 0 = away, 1 = toward
        let alpha = 0.35 + 0.65 * toward;
        background.0 = axis_color(ball.axis).with_alpha(alpha);
        *z = ZIndex(if facing >= 0.0 { 1 } else { 0 });
    }
}
