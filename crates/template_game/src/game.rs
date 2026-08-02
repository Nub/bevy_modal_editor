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
        app.init_resource::<GameInputActive>()
            .add_systems(Startup, leave_boot)
            .add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(OnExit(AppState::MainMenu), despawn_menu)
            .add_systems(Update, menu_start.run_if(in_state(AppState::MainMenu)))
            .add_systems(OnEnter(AppState::LoadingLevel), spawn_level)
            .add_systems(
                Update,
                (player_look, player_walk, sync_cursor_grab)
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
    let box_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.45, 0.35),
        ..default()
    });

    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(80.0, 80.0))),
        MeshMaterial3d(ground),
        Transform::IDENTITY,
    ));
    for (x, z, h) in [(4.0, -6.0, 1.0), (-5.0, -3.0, 2.0), (0.0, -10.0, 1.5), (7.0, 2.0, 0.5)] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(2.0, h * 2.0, 2.0))),
            MeshMaterial3d(box_mat.clone()),
            Transform::from_xyz(x, h, z),
        ));
    }
    commands.spawn((
        DirectionalLight { illuminance: 8_000.0, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
    commands.spawn((
        Player { yaw: 0.0, pitch: 0.0 },
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
        player.pitch = (player.pitch - motion.delta.y * LOOK_SENSITIVITY)
            .clamp(-1.54, 1.54);
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
        if keys.pressed(KeyCode::KeyW) { wish += flat_forward; }
        if keys.pressed(KeyCode::KeyS) { wish -= flat_forward; }
        if keys.pressed(KeyCode::KeyD) { wish += flat_right; }
        if keys.pressed(KeyCode::KeyA) { wish -= flat_right; }
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
    let mut cursor = cursor.into_inner();
    let (grab, visible) = if input_active.0 {
        (CursorGrabMode::Locked, false)
    } else {
        (CursorGrabMode::None, true)
    };
    if cursor.grab_mode != grab {
        cursor.grab_mode = grab;
        cursor.visible = visible;
    }
}
