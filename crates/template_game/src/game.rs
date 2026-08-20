//! The game itself: menu → load level → first-person walk. Editor-free by
//! construction — this module must compile identically with and without the
//! `editor` feature; the overlay only reads/writes `GameInputActive`.

use avian3d::prelude::*;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use game_framework::{AppState, primitive_mesh};

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
#[reflect(Component, Default)]
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
    pub ground: Handle<StandardMaterial>,
}

fn init_primitive_assets(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(PrimitiveAssets {
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.45, 0.35),
            ..default()
        }),
        ground: materials.add(StandardMaterial {
            base_color: Color::srgb(0.35, 0.38, 0.35),
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
        PrimitiveKind::Cube => meshes.add(primitive_mesh(Cuboid::new(size, size, size))),
        PrimitiveKind::Sphere => meshes.add(primitive_mesh(Sphere::new(size * 0.5))),
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
        app.register_type::<PhysicsBody>();
        app.register_type::<AutoBoxCollider>();
        app.register_type::<Ground>();
        app.register_type::<PlayerSpawn>();
        // Physics is part of the GAME (avian3d). While the editor owns input
        // the simulation is paused — dynamic props hold still under editing
        // and only live during play.
        app.add_plugins(PhysicsPlugins::default());
        app.init_resource::<GameInputActive>()
            .add_systems(Startup, (init_primitive_assets, leave_boot))
            .add_systems(Update, (derive_physics, derive_auto_colliders))
            .add_observer(on_primitive_added)
            .add_observer(on_ground_added)
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
#[reflect(Component, Default)]
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

/// Data-driven collision volume (M4-D12 game-ready flow): the game declares
/// its collider as plain reflected data on the entity — the physics system of
/// a real game consumes it; here it exists so imported models become GAME
/// CONTENT (mesh + collider + behavior) with zero editor code.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug)]
#[reflect(Component, Default)]
pub struct BoxCollider {
    pub half_extents: Vec3,
    /// Collider center relative to the entity (mesh bounds rarely sit on the
    /// origin — Fit Collider writes this).
    #[reflect(default)]
    pub offset: Vec3,
}

impl Default for BoxCollider {
    fn default() -> Self {
        Self {
            half_extents: Vec3::splat(0.5),
            offset: Vec3::ZERO,
        }
    }
}

/// How the physics engine treats this entity (editor-authored DATA — avian
/// components derive from it at runtime, they never serialize themselves).
#[derive(Component, Reflect, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[reflect(Component, Default)]
pub enum PhysicsBody {
    /// Immovable level geometry (walls, floors).
    #[default]
    Static,
    /// Simulated: falls, collides, stacks (barrels, props).
    Dynamic,
}

/// Fit a box to whatever the entity actually RENDERS, and keep it fitted.
///
/// `BoxCollider` is a fixed volume you author once; this is the declarative
/// form — no numbers to type, and it re-fits when the content changes. That
/// matters most for imported models, whose geometry lives in the derived gltf
/// children (the entity you select has no mesh of its own) and arrives
/// asynchronously, so a one-shot fit at placement time would measure nothing.
///
/// Takes precedence over `BoxCollider` on the same entity — one of them owns
/// the derived collider, and the automatic one is the more specific intent.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug, Default)]
#[reflect(Component, Default)]
pub struct AutoBoxCollider;

/// What `AutoBoxCollider` last fitted — refit only when the bounds actually
/// move, so a settled model costs one comparison a frame, not a respawned
/// collider.
#[derive(Component)]
pub struct AutoFitted {
    pub half_extents: Vec3,
    pub offset: Vec3,
}

/// Where the player starts, as an ordinary scene entity — selectable,
/// movable, and saved with the level. It has no geometry, so the editor draws
/// it from the gizmo the game registers (see `GameFeature::register`).
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug, Default)]
#[reflect(Component, Default)]
pub struct PlayerSpawn {
    /// Eye height above the spawn transform.
    pub eye_height: f32,
}

/// The ground plane as DATA (spec §5 marker/regenerate), so it is an ordinary
/// scene entity: visible in the hierarchy, selectable, resizable from the
/// inspector, and it survives a save/load round trip. Hardcoding it in
/// `spawn_level` made it invisible to the editor — you could stand on it but
/// never click it.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug)]
#[reflect(Component, Default)]
pub struct Ground {
    /// Side length of the square floor, in metres.
    pub size: f32,
}

impl Default for Ground {
    fn default() -> Self {
        Self { size: 80.0 }
    }
}

/// Regenerate: mesh, material and collider derive from `Ground` — in editor, in
/// game, on load, on undo, identically.
fn on_ground_added(
    add: On<bevy::ecs::lifecycle::Add, Ground>,
    grounds: Query<&Ground>,
    assets: Res<PrimitiveAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let entity = add.entity;
    let Ok(ground) = grounds.get(entity) else {
        return;
    };
    let size = if ground.size > 0.0 { ground.size } else { 80.0 };
    commands.entity(entity).insert((
        Mesh3d(meshes.add(primitive_mesh(Plane3d::default().mesh().size(size, size)))),
        MeshMaterial3d(assets.ground.clone()),
        // The floor every dynamic prop lands on.
        RigidBody::Static,
        Collider::cuboid(size, 0.1, size),
    ));
}

/// Marks a subtree that is DECORATION, not content: it renders, but it is not
/// part of the shape a collider should hug. Editor gizmos (socket cones and the
/// like) get this so a piece's collider does not grow to swallow them — the
/// game module stays editor-free, and whoever spawns the decoration says so.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug, Default)]
#[reflect(Component, Default)]
pub struct BoundsIgnored;

/// The tight box around everything `root` renders, in ROOT-LOCAL space,
/// derived-gltf children included. `None` while nothing has bounds yet — an
/// imported model's meshes land frames after the entity does.
pub fn visual_bounds(
    root: Entity,
    root_global: &GlobalTransform,
    children: &Query<&Children>,
    aabbs: &Query<(&bevy::camera::primitives::Aabb, &GlobalTransform)>,
    ignored: &Query<(), With<BoundsIgnored>>,
) -> Option<(Vec3, Vec3)> {
    let to_local = root_global.affine().inverse();
    let mut min = Vec3::MAX;
    let mut max = Vec3::MIN;
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        // Decoration contributes nothing, and neither does anything under it.
        if entity != root && ignored.contains(entity) {
            continue;
        }
        if let Ok((aabb, global)) = aabbs.get(entity) {
            let center = Vec3::from(aabb.center);
            let he = Vec3::from(aabb.half_extents);
            for corner in 0..8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                let local = to_local.transform_point3(global.transform_point(center + he * sign));
                min = min.min(local);
                max = max.max(local);
            }
        }
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
    }
    (min.x <= max.x).then(|| ((max - min) * 0.5, (max + min) * 0.5))
}

/// Derived marker: the avian collider child a `BoxCollider` spawns. Never
/// serialized — rebuilt from the data component like every derived visual.
#[derive(Component)]
pub struct ColliderDerived;

/// Replace `entity`'s derived collider with a box of these dimensions.
fn respawn_collider(
    commands: &mut Commands,
    entity: Entity,
    half_extents: Vec3,
    offset: Vec3,
    body: Option<PhysicsBody>,
    children: &Query<&Children>,
    derived: &Query<(), With<ColliderDerived>>,
) {
    if let Ok(kids) = children.get(entity) {
        for kid in kids.iter() {
            if derived.contains(kid) {
                commands.entity(kid).despawn();
            }
        }
    }
    let rigid = match body.unwrap_or_default() {
        PhysicsBody::Static => RigidBody::Static,
        PhysicsBody::Dynamic => RigidBody::Dynamic,
    };
    commands.entity(entity).insert(rigid);
    commands.spawn((
        ColliderDerived,
        Collider::cuboid(
            half_extents.x * 2.0,
            half_extents.y * 2.0,
            half_extents.z * 2.0,
        ),
        Transform::from_translation(offset),
        ChildOf(entity),
    ));
}

/// `AutoBoxCollider` → a fitted collider, kept in sync with the content.
/// Polls rather than reacting to change detection: mesh bounds arrive with the
/// asset load, which no component on this entity reports.
#[allow(clippy::type_complexity)]
fn derive_auto_colliders(
    autos: Query<
        (
            Entity,
            &GlobalTransform,
            Option<&PhysicsBody>,
            Option<&AutoFitted>,
        ),
        With<AutoBoxCollider>,
    >,
    children: Query<&Children>,
    aabbs: Query<(&bevy::camera::primitives::Aabb, &GlobalTransform)>,
    ignored: Query<(), With<BoundsIgnored>>,
    derived: Query<(), With<ColliderDerived>>,
    mut removed: RemovedComponents<AutoBoxCollider>,
    bodies: Query<(), With<RigidBody>>,
    mut commands: Commands,
) {
    for entity in removed.read() {
        if bodies.contains(entity) {
            commands.entity(entity).remove::<RigidBody>();
        }
        if let Ok(kids) = children.get(entity) {
            for kid in kids.iter() {
                if derived.contains(kid) {
                    commands.entity(kid).despawn();
                }
            }
        }
        if let Ok(mut e) = commands.get_entity(entity) {
            e.remove::<AutoFitted>();
        }
    }
    for (entity, global, body, fitted) in &autos {
        // Bounds include OUR collider child's transform-only entity, which has
        // no Aabb — nothing to exclude, so the measurement stays stable.
        let Some((half_extents, offset)) =
            visual_bounds(entity, global, &children, &aabbs, &ignored)
        else {
            continue; // still loading — try again next frame
        };
        let settled = fitted.is_some_and(|f| {
            f.half_extents.abs_diff_eq(half_extents, 1e-4) && f.offset.abs_diff_eq(offset, 1e-4)
        });
        if settled {
            continue;
        }
        respawn_collider(
            &mut commands,
            entity,
            half_extents,
            offset,
            body.copied(),
            &children,
            &derived,
        );
        commands.entity(entity).insert(AutoFitted {
            half_extents,
            offset,
        });
    }
}

/// Editor owns input → physics holds still; play → simulate.
pub fn sync_physics_pause_now(game_input: Res<GameInputActive>, mut time: ResMut<Time<Physics>>) {
    if game_input.0 {
        if time.is_paused() {
            time.unpause();
        }
    } else if !time.is_paused() {
        time.pause();
    }
}

/// DATA → avian: `BoxCollider`/`PhysicsBody` (serialized, editor-authored)
/// derive the runtime `RigidBody` + collider child. Re-derives on any edit;
/// removal (undo) strips the runtime state.
#[allow(clippy::type_complexity)]
fn derive_physics(
    changed: Query<
        (Entity, &BoxCollider, Option<&PhysicsBody>),
        (
            Or<(Changed<BoxCollider>, Changed<PhysicsBody>)>,
            // The automatic fit owns the collider where both are present.
            Without<AutoBoxCollider>,
        ),
    >,
    children: Query<&Children>,
    derived: Query<(), With<ColliderDerived>>,
    mut removed_colliders: RemovedComponents<BoxCollider>,
    bodies: Query<(), With<RigidBody>>,
    autos: Query<(), With<AutoBoxCollider>>,
    mut commands: Commands,
) {
    for entity in removed_colliders.read() {
        // An auto-fitted entity keeps its collider when the manual one goes.
        if autos.contains(entity) {
            continue;
        }
        if bodies.contains(entity) {
            commands.entity(entity).remove::<RigidBody>();
        }
        if let Ok(kids) = children.get(entity) {
            for kid in kids.iter() {
                if derived.contains(kid) {
                    commands.entity(kid).despawn();
                }
            }
        }
    }
    for (entity, collider, body) in &changed {
        respawn_collider(
            &mut commands,
            entity,
            collider.half_extents,
            collider.offset,
            body.copied(),
            &children,
            &derived,
        );
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
fn spawn_level(mut commands: Commands, mut next: ResMut<NextState<AppState>>) {
    // Scene entities like everything else (owner: the floor and the sun must be
    // selectable) — mesh, material and collider derive from the data.
    commands.spawn((
        #[cfg(feature = "editor")]
        editor_api::prelude::SceneId::random(),
        Ground::default(),
        Name::new("Ground"),
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
        #[cfg(feature = "editor")]
        editor_api::prelude::SceneId::random(),
        DirectionalLight {
            illuminance: 8_000.0,
            ..default()
        },
        Name::new("Sun"),
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.9, 0.4, 0.0)),
    ));
    // The LOOK is authored data too: "this room glows" is a property of the
    // room, not of a camera that only exists while someone is playing. Cameras
    // adopt it (see `game_framework::adopt_authored_look`), which also means
    // the sequencer can keyframe the level's bloom and every camera follows.
    commands.spawn((
        #[cfg(feature = "editor")]
        editor_api::prelude::SceneId::random(),
        game_framework::PostProcess::default(),
        Name::new("Post Process"),
    ));
    // An emitter waiting for its cue. Every number on it is a track address, so
    // the burst itself can be animated — a fountain that widens is two keys.
    commands.spawn((
        #[cfg(feature = "editor")]
        editor_api::prelude::SceneId::random(),
        game_framework::Burst::default(),
        Name::new("Burst"),
        Transform::from_xyz(0.0, 1.0, 0.0),
    ));
    // The spawn POINT is authored data; the player camera derives from it, so
    // moving the widget moves where you start.
    let spawn = Transform::from_xyz(0.0, 0.0, 6.0);
    let eye_height = 1.7;
    commands.spawn((
        #[cfg(feature = "editor")]
        editor_api::prelude::SceneId::random(),
        PlayerSpawn { eye_height },
        Name::new("Player Spawn"),
        spawn,
    ));
    commands.spawn((
        Player {
            yaw: 0.0,
            pitch: 0.0,
        },
        Camera3d::default(),
        Transform::from_translation(spawn.translation + Vec3::Y * eye_height)
            .with_rotation(spawn.rotation),
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
