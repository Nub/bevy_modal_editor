//! `game_framework` — the opinionated patterns a game follows (spec §3, §9).
//!
//! Never depends on any `editor_*` crate. The editor understands these states; games
//! and feature crates gate their systems on them instead of inventing parallel flags.
//!
//! M1 fleshes this out (sub-states, session/connection flow on lightyear semantics,
//! level-loading service, settings). This skeleton exists so `template_game` boots
//! through the canonical lifecycle from the first commit.

use bevy::prelude::*;

/// Top-level application lifecycle (spec §3). Sub-states (`Session`, connection state)
/// arrive in M1.
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Boot,
    MainMenu,
    LoadingLevel,
    InGame,
}

pub struct GameFrameworkPlugin;

impl Plugin for GameFrameworkPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>();
        app.register_type::<PostProcess>();
        app.add_systems(Update, (adopt_authored_look, apply_post_process).chain());
        app.register_type::<Burst>();
        app.register_type::<Particle>();
        app.add_message::<FireEffect>();
        app.add_systems(Update, (fire_bursts, age_particles));
    }
}

/// Build a primitive mesh WITH tangents.
///
/// Bevy compiles the whole normal-mapping branch out unless a mesh's vertex
/// layout carries `ATTRIBUTE_TANGENT` (the PBR shader gates it behind
/// `#ifdef VERTEX_TANGENTS`), and no primitive builder emits one. A normal map
/// on a greybox cube is therefore discarded in silence — no warning, no error,
/// just a surface that never changes. glTF meshes arrive with tangents from the
/// importer; anything a game generates has to ask.
pub fn primitive_mesh(shape: impl Into<Mesh>) -> Mesh {
    let mut mesh = shape.into();
    if let Err(error) = mesh.generate_tangents() {
        // Not fatal: the surface still shades, it just cannot show a normal map.
        warn!("no tangents for a primitive mesh; normal maps will not show: {error}");
    }
    mesh
}

/// The post-process look of a camera, as game data (spec §9 effects layer).
///
/// This is a plain component the GAME owns, not editor state, and that is the
/// point: it ships in a release build, it serializes with the scene, the
/// inspector edits it because it is registered like any other component, and
/// the sequencer can keyframe it because a keyframe is a field address and a
/// number. Bloom over two seconds is an effect nobody had to write a system for.
///
/// It describes intent — how bright the glow, how exposed the image — and a
/// system below turns that into the render components Bevy actually reads, so
/// the game never hand-manages `Bloom`.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug)]
#[reflect(Component, Default)]
pub struct PostProcess {
    /// How much light spills out of bright things. 0 turns bloom off entirely
    /// rather than leaving an imperceptible pass running.
    pub bloom: f32,
    /// Exposure in EV100. Higher is DARKER, as on a camera: it is the exposure
    /// value, not a brightness knob, and calling it what it is avoids a
    /// slider whose direction has to be memorised.
    pub ev100: f32,
}

impl Default for PostProcess {
    fn default() -> Self {
        Self {
            bloom: 0.0,
            // Bevy's own indoor default, so adding the component changes
            // nothing until someone asks it to.
            ev100: 9.7,
        }
    }
}

/// Cameras adopt the level's authored look.
///
/// A camera exists only while someone is playing; the look belongs to the room.
/// Copying rather than referencing keeps the render path reading ONE component,
/// and it means editing the authored entity — or keyframing it — updates every
/// camera the next frame.
fn adopt_authored_look(
    mut commands: Commands,
    authored: Query<&PostProcess, Without<bevy::camera::Camera>>,
    cameras: Query<(Entity, Option<&PostProcess>), With<bevy::camera::Camera>>,
) {
    let Some(look) = authored.iter().next() else {
        return;
    };
    for (entity, current) in &cameras {
        if current != Some(look) {
            commands.entity(entity).insert(*look);
        }
    }
}

/// Turn the intent into the render components Bevy reads.
fn apply_post_process(
    mut commands: Commands,
    cameras: Query<
        (
            Entity,
            &PostProcess,
            Option<&bevy::post_process::bloom::Bloom>,
        ),
        Changed<PostProcess>,
    >,
) {
    for (entity, post, existing) in &cameras {
        let mut camera = commands.entity(entity);
        camera.insert(bevy::camera::Exposure { ev100: post.ev100 });
        if post.bloom <= 0.0 {
            if existing.is_some() {
                camera.remove::<bevy::post_process::bloom::Bloom>();
            }
            continue;
        }
        camera.insert(bevy::post_process::bloom::Bloom {
            intensity: post.bloom,
            ..bevy::post_process::bloom::Bloom::NATURAL
        });
    }
}

#[cfg(test)]
mod post_tests {
    use super::*;

    fn app_with(post: PostProcess) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, apply_post_process);
        let camera = app.world_mut().spawn(post).id();
        app.update();
        (app, camera)
    }

    // Intent becomes the render components Bevy actually reads, so a game never
    // hand-manages Bloom.
    #[test]
    fn bloom_intent_becomes_a_bloom_component() {
        let (app, camera) = app_with(PostProcess {
            bloom: 0.4,
            ..Default::default()
        });
        let bloom = app
            .world()
            .get::<bevy::post_process::bloom::Bloom>(camera)
            .expect("a bloom pass exists");
        assert!(
            (bloom.intensity - 0.4).abs() < 1e-5,
            "at the asked intensity"
        );
    }

    // Zero means OFF, not "a pass running imperceptibly": a post-process stack
    // that cannot be turned off is a permanent frame cost.
    #[test]
    fn zero_bloom_removes_the_pass_entirely() {
        let (mut app, camera) = app_with(PostProcess {
            bloom: 0.5,
            ..Default::default()
        });
        assert!(
            app.world()
                .get::<bevy::post_process::bloom::Bloom>(camera)
                .is_some()
        );

        app.world_mut()
            .get_mut::<PostProcess>(camera)
            .unwrap()
            .bloom = 0.0;
        app.update();
        assert!(
            app.world()
                .get::<bevy::post_process::bloom::Bloom>(camera)
                .is_none(),
            "the pass is gone, not merely quiet"
        );
    }

    // Exposure is EV100 — higher is DARKER, as on a camera. Naming it for what
    // it is beats a brightness slider whose direction has to be memorised.
    #[test]
    fn exposure_reaches_the_camera_as_ev100() {
        let (app, camera) = app_with(PostProcess {
            ev100: 12.5,
            ..Default::default()
        });
        let exposure = app
            .world()
            .get::<bevy::camera::Exposure>(camera)
            .expect("the camera carries an exposure");
        assert!((exposure.ev100 - 12.5).abs() < 1e-5);
    }

    // The default changes nothing: adding the component to an existing camera
    // must not alter how the game already looks.
    #[test]
    fn the_default_adds_no_bloom() {
        let (app, camera) = app_with(PostProcess::default());
        assert!(
            app.world()
                .get::<bevy::post_process::bloom::Bloom>(camera)
                .is_none(),
            "no glow until asked for"
        );
    }
}

#[cfg(test)]
mod adopt_tests {
    use super::*;

    // Editing the level's look reaches every camera — including, therefore,
    // keyframing it, since a track just writes the same field.
    #[test]
    fn cameras_adopt_the_authored_look() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_systems(Update, (adopt_authored_look, apply_post_process).chain());
        let level = app
            .world_mut()
            .spawn(PostProcess {
                bloom: 0.6,
                ev100: 10.0,
            })
            .id();
        let camera = app.world_mut().spawn(bevy::camera::Camera::default()).id();
        app.update();

        let adopted = app.world().get::<PostProcess>(camera).copied();
        assert_eq!(
            adopted.map(|look| look.bloom),
            Some(0.6),
            "the camera took the room's look"
        );

        // And FOLLOWS it: this is what makes a keyframed bloom work at all.
        app.world_mut().get_mut::<PostProcess>(level).unwrap().bloom = 0.1;
        app.update();
        app.update();
        assert_eq!(
            app.world().get::<PostProcess>(camera).map(|l| l.bloom),
            Some(0.1),
            "and follows it when it changes"
        );
    }
}

/// Ask for a named effect. The sequencer's events arrive as these, but so can
/// anything else — a collision, a pickup, a rule firing — which is the point:
/// effects are triggered by NAME, and nothing that triggers one needs to know
/// what it looks like.
#[derive(Message, Clone, Debug)]
pub struct FireEffect {
    pub name: String,
}

/// A particle burst, authored on an entity, waiting for its cue.
///
/// Every field is a number a track can address, so the burst itself is
/// animatable: a fountain that widens over ten seconds is two keys, not a new
/// component. It lives here rather than in the editor because it has to exist
/// in a release build — the editor edits it, the game runs it.
#[derive(Component, Reflect, Clone, PartialEq, Debug)]
#[reflect(Component, Default)]
pub struct Burst {
    /// The cue. Matching on a name is the whole interface.
    pub event: String,
    pub count: u32,
    /// Metres per second, before gravity.
    pub speed: f32,
    /// Seconds each particle lives. Nothing outlives its burst.
    pub lifetime: f32,
    pub size: f32,
    /// How much the spray is pulled down. 0 for sparks in space.
    pub gravity: f32,
}

impl Default for Burst {
    fn default() -> Self {
        Self {
            event: "burst".into(),
            count: 24,
            speed: 4.0,
            lifetime: 0.9,
            size: 0.08,
            gravity: 9.8,
        }
    }
}

/// One particle in flight. Despawned by `age_particles` when it runs out.
///
/// Reflected so tools can SEE it. It is deliberately not an editor component:
/// particles are transient and must never be selected, saved, or keyed.
#[derive(Component, Reflect, Clone, Copy, Debug)]
#[reflect(Component)]
pub struct Particle {
    pub velocity: Vec3,
    pub remaining: f32,
    pub gravity: f32,
}

/// Directions spread over a sphere by the golden angle.
///
/// Deterministic on purpose: a burst that looks different every run cannot be
/// tested, and a designer tuning a fountain wants the change they made to be
/// the only thing that moved. It is also, conveniently, more even than random.
pub fn burst_direction(index: u32, count: u32) -> Vec3 {
    let count = count.max(1) as f32;
    let i = index as f32;
    // Fibonacci sphere: y walks the axis, the angle turns by the golden angle.
    let y = 1.0 - (i / count) * 2.0;
    let radius = (1.0 - y * y).max(0.0).sqrt();
    let theta = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt()) * i;
    Vec3::new(theta.cos() * radius, y, theta.sin() * radius)
}

fn fire_bursts(
    mut commands: Commands,
    mut effects: MessageReader<FireEffect>,
    emitters: Query<(&Burst, &GlobalTransform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let fired: Vec<String> = effects.read().map(|effect| effect.name.clone()).collect();
    if fired.is_empty() {
        return;
    }
    for (burst, at) in &emitters {
        if !fired.contains(&burst.event) {
            continue;
        }
        // Unlit: a spark is a light source, not a surface being lit, and an
        // unlit quad reads as a spark at any size.
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.5),
            unlit: true,
            ..default()
        });
        let mesh = meshes.add(primitive_mesh(Sphere::new(burst.size.max(0.001))));
        let origin = at.translation();
        for index in 0..burst.count {
            commands.spawn((
                Particle {
                    velocity: burst_direction(index, burst.count) * burst.speed,
                    remaining: burst.lifetime,
                    gravity: burst.gravity,
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(origin),
            ));
        }
    }
}

fn age_particles(
    mut commands: Commands,
    time: Option<Res<Time>>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform)>,
) {
    let Some(time) = time else { return };
    let delta = time.delta_secs();
    for (entity, mut particle, mut transform) in &mut particles {
        particle.remaining -= delta;
        if particle.remaining <= 0.0 {
            // Nothing outlives its burst: a particle system that leaks entities
            // is a memory leak with a pretty face.
            commands.entity(entity).despawn();
            continue;
        }
        let gravity = particle.gravity;
        particle.velocity.y -= gravity * delta;
        let step = particle.velocity * delta;
        transform.translation += step;
    }
}

#[cfg(test)]
mod burst_tests {
    use super::*;

    fn burst_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.add_message::<FireEffect>();
        app.add_systems(Update, (fire_bursts, age_particles));
        app
    }

    // The cue is a NAME: an emitter fires for its own event and stays put for
    // anything else, which is what lets one timeline drive many effects.
    #[test]
    fn an_emitter_fires_only_for_its_own_event() {
        let mut app = burst_app();
        app.world_mut().spawn((
            Burst {
                event: "sparks".into(),
                count: 8,
                ..Default::default()
            },
            GlobalTransform::default(),
        ));

        app.world_mut().write_message(FireEffect {
            name: "something else".into(),
        });
        app.update();
        assert_eq!(count_particles(&mut app), 0, "not its cue");

        app.world_mut().write_message(FireEffect {
            name: "sparks".into(),
        });
        app.update();
        assert_eq!(count_particles(&mut app), 8, "its cue, its count");
    }

    fn count_particles(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&Particle>().iter(world).count()
    }

    // Nothing outlives its burst. A particle system that leaks entities is a
    // memory leak with a pretty face.
    #[test]
    fn particles_expire() {
        let mut app = burst_app();
        app.world_mut().spawn((
            Burst {
                event: "puff".into(),
                count: 4,
                lifetime: 0.05,
                ..Default::default()
            },
            GlobalTransform::default(),
        ));
        app.world_mut().write_message(FireEffect {
            name: "puff".into(),
        });
        app.update();
        assert_eq!(count_particles(&mut app), 4);

        // Long enough for the lifetime to pass, whatever the tick was.
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.update();
        }
        assert_eq!(count_particles(&mut app), 0, "all gone");
    }

    // The spread covers a sphere rather than clumping: every direction is a
    // unit vector, and opposite indices genuinely point apart.
    #[test]
    fn the_spread_covers_a_sphere() {
        let count = 64;
        for index in 0..count {
            let direction = burst_direction(index, count);
            assert!(
                (direction.length() - 1.0).abs() < 1e-3,
                "index {index} is a unit direction, got {}",
                direction.length()
            );
        }
        let first = burst_direction(0, count);
        let last = burst_direction(count - 1, count);
        assert!(
            first.dot(last) < -0.9,
            "the ends of the spiral point opposite ways: {first:?} {last:?}"
        );
    }

    // Deterministic: a burst that looks different every run cannot be tested,
    // and a designer tuning one wants their change to be the only thing moving.
    #[test]
    fn the_spread_is_the_same_every_time() {
        let once: Vec<Vec3> = (0..16).map(|i| burst_direction(i, 16)).collect();
        let again: Vec<Vec3> = (0..16).map(|i| burst_direction(i, 16)).collect();
        assert_eq!(once, again);
    }
}
