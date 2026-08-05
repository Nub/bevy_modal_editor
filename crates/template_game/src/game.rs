//! The game itself: menu → load level → first-person walk. Editor-free by
//! construction — this module must compile identically with and without the
//! `editor` feature; the overlay only reads/writes `GameInputActive`.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use game_framework::AppState;

/// Whether the game consumes player input this frame. The editor overlay (when
/// compiled in and active) sets this false; without the editor it is always true.
#[derive(Resource)]
pub struct GameInputActive(pub bool);

impl Default for GameInputActive {
    fn default() -> Self {
        Self(true)
    }
}

/// Semantic scene primitive: the SERIALIZED truth. Meshes/materials derive from it
/// via the regenerate observer (spec §5 marker/regenerate pattern) — in editor, in
/// game, on load, on undo, identically.
#[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
#[reflect(Component)]
pub struct Primitive {
    pub kind: PrimitiveKind,
    pub size: f32,
}

#[derive(Reflect, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimitiveKind {
    #[default]
    Cube,
    Sphere,
}

#[derive(Resource)]
pub struct PrimitiveAssets {
    pub material: Handle<StandardMaterial>,
}

fn init_primitive_assets(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(PrimitiveAssets {
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.45, 0.35),
            ..default()
        }),
    });
}

/// Regenerate: derive render state from the semantic component. One path shared by
/// level spawn, editor placement, scene load, and undo respawn.
fn on_primitive_added(
    add: On<bevy::ecs::lifecycle::Add, Primitive>,
    primitives: Query<&Primitive>,
    assets: Res<PrimitiveAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let entity = add.entity;
    let Ok(primitive) = primitives.get(entity) else {
        return;
    };
    let size = if primitive.size > 0.0 {
        primitive.size
    } else {
        1.0
    };
    let mesh = match primitive.kind {
        PrimitiveKind::Cube => meshes.add(Cuboid::new(size, size, size)),
        PrimitiveKind::Sphere => meshes.add(Sphere::new(size * 0.5)),
    };
    commands
        .entity(entity)
        .insert((Mesh3d(mesh), MeshMaterial3d(assets.material.clone())));
}

#[derive(Component)]
pub struct Player {
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Component)]
struct MenuUi;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Primitive>();
        app.register_type::<Spinner>();
        app.init_resource::<GameInputActive>()
            .add_systems(Startup, (init_primitive_assets, leave_boot))
            .add_observer(on_primitive_added)
            .add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(OnExit(AppState::MainMenu), despawn_menu)
            .add_systems(Update, menu_start.run_if(in_state(AppState::MainMenu)))
            .add_systems(OnEnter(AppState::LoadingLevel), spawn_level)
            .add_systems(
                Update,
                (player_look, player_walk, sync_cursor_grab, spin)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn leave_boot(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::MainMenu);
}

fn spawn_menu(mut commands: Commands) {
    commands.spawn((Camera2d, MenuUi));
    commands.spawn((
        MenuUi,
        Text::new("TEMPLATE GAME\n\npress Enter to play"),
        Node {
            position_type: PositionType::Absolute,
            left: bevy::ui::px(40),
            top: bevy::ui::px(40),
            ..default()
        },
    ));
}

fn despawn_menu(mut commands: Commands, menu: Query<Entity, With<MenuUi>>) {
    for e in &menu {
        commands.entity(e).despawn();
    }
}

fn menu_start(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<AppState>>) {
    if keys.just_pressed(KeyCode::Enter) {
        next.set(AppState::LoadingLevel);
    }
}

/// A DATA-DRIVEN gameplay component (M3-C7, spec §"designer surface"): plain
/// reflected data the game registers like any component — the editor needs zero
/// code for it (inspector edits, undo, serialization all fall out of registration).
/// Enabled boxes spin during play; designers tune it entirely from the inspector.
#[derive(Component, Reflect, Clone, PartialEq, Debug)]
#[reflect(Component)]
pub struct Spinner {
    pub enabled: bool,
    pub degrees_per_sec: f32,
}

impl Default for Spinner {
    fn default() -> Self {
        Self {
            enabled: false,
            degrees_per_sec: 45.0,
        }
    }
}

/// Gameplay behavior for `Spinner` — runs only while the game owns input, so
/// editing values in the editor never fights a live rotation.
fn spin(
    game_input: Res<GameInputActive>,
    time: Res<Time>,
    mut spinners: Query<(&Spinner, &mut Transform)>,
) {
    if !game_input.0 {
        return;
    }
    for (spinner, mut transform) in &mut spinners {
        if spinner.enabled {
            transform.rotate_y(spinner.degrees_per_sec.to_radians() * time.delta_secs());
        }
    }
}

/// M1 graybox: ground, some boxes, a light, the player camera. Loading is synchronous
/// here; the real async level service arrives in later milestones.
fn spawn_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut next: ResMut<NextState<AppState>>,
) {
    let ground = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.38, 0.35),
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(80.0, 80.0))),
        MeshMaterial3d(ground),
        Transform::IDENTITY,
    ));
    // Graybox content: SEMANTIC scene entities — meshes derive via the observer, and
    // (with the editor feature) these are selectable, movable, savable.
    for (x, z, size) in [
        (4.0, -6.0, 2.0),
        (-5.0, -3.0, 3.0),
        (0.0, -10.0, 2.5),
        (7.0, 2.0, 1.5),
    ] {
        commands.spawn((
            #[cfg(feature = "editor")]
            editor_api::prelude::SceneId::random(),
            Primitive {
                kind: PrimitiveKind::Cube,
                size,
            },
            Spinner::default(),
            Name::new("Box"),
            Transform::from_xyz(x, size / 2.0, z),
        ));
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
    commands.spawn((
        Player {
            yaw: 0.0,
            pitch: 0.0,
        },
        Camera3d::default(),
        Transform::from_xyz(0.0, 1.7, 6.0),
    ));

    next.set(AppState::InGame);
}

const LOOK_SENSITIVITY: f32 = 0.0025;
const WALK_SPEED: f32 = 6.0;

fn player_look(
    input_active: Res<GameInputActive>,
    motion: Res<AccumulatedMouseMotion>,
    mut player: Query<(&mut Player, &mut Transform)>,
) {
    if !input_active.0 {
        return;
    }
    for (mut player, mut transform) in &mut player {
        player.yaw -= motion.delta.x * LOOK_SENSITIVITY;
        player.pitch = (player.pitch - motion.delta.y * LOOK_SENSITIVITY).clamp(-1.54, 1.54);
        transform.rotation =
            Quat::from_rotation_y(player.yaw) * Quat::from_rotation_x(player.pitch);
    }
}

fn player_walk(
    input_active: Res<GameInputActive>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut player: Query<&mut Transform, With<Player>>,
) {
    if !input_active.0 {
        return;
    }
    for mut transform in &mut player {
        let mut wish = Vec3::ZERO;
        let forward = transform.forward();
        let flat_forward = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
        let right = transform.right();
        let flat_right = Vec3::new(right.x, 0.0, right.z).normalize_or_zero();
        if keys.pressed(KeyCode::KeyW) {
            wish += flat_forward;
        }
        if keys.pressed(KeyCode::KeyS) {
            wish -= flat_forward;
        }
        if keys.pressed(KeyCode::KeyD) {
            wish += flat_right;
        }
        if keys.pressed(KeyCode::KeyA) {
            wish -= flat_right;
        }
        let step = wish.normalize_or_zero() * WALK_SPEED * time.delta_secs();
        transform.translation += step;
        transform.translation.y = 1.7; // stay on the floor (no physics in M1)
    }
}

/// Cursor lock follows game-input ownership: locked while the game owns input,
/// released when the editor overlay takes over (or in menus).
fn sync_cursor_grab(
    input_active: Res<GameInputActive>,
    cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    // The game only asserts cursor policy while it OWNS input; when the editor is
    // active, the editor (fly-nav) owns the cursor.
    if !input_active.0 {
        return;
    }
    let mut cursor = cursor.into_inner();
    if cursor.grab_mode != CursorGrabMode::Locked {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // C7: the data-driven component DRIVES gameplay — enabled spinners rotate
    // while the game owns input, and never under the editor.
    #[test]
    fn spinner_rotates_only_when_game_active() {
        let mut app = App::new();
        app.init_resource::<GameInputActive>();
        app.insert_resource(Time::<()>::default());
        app.add_systems(Update, spin);
        let entity = app
            .world_mut()
            .spawn((
                Spinner {
                    enabled: true,
                    degrees_per_sec: 90.0,
                },
                Transform::IDENTITY,
            ))
            .id();

        // Editor owns input: no rotation.
        app.world_mut().resource_mut::<GameInputActive>().0 = false;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        app.update();
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().rotation,
            Quat::IDENTITY
        );

        // Game owns input: ~90° after one second.
        app.world_mut().resource_mut::<GameInputActive>().0 = true;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        app.update();
        let (_, angle) = app
            .world()
            .get::<Transform>(entity)
            .unwrap()
            .rotation
            .to_axis_angle();
        assert!(
            (angle.to_degrees() - 90.0).abs() < 1.0,
            "angle {}",
            angle.to_degrees()
        );

        // Disabled: rotation freezes.
        app.world_mut().get_mut::<Spinner>(entity).unwrap().enabled = false;
        let before = app.world().get::<Transform>(entity).unwrap().rotation;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        app.update();
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().rotation,
            before
        );
    }
}
