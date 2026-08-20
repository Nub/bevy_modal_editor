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
