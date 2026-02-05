use avian3d::debug_render::PhysicsDebugPlugin;
use avian3d::prelude::{Physics, PhysicsPlugins};
use avian3d::schedule::PhysicsTime;
use bevy::prelude::*;
use bevy_egui::EguiPlugin;
use bevy_grid_shader::GridMaterialPlugin;
use bevy_outliner::prelude::*;
use bevy_spline_3d::distribution::SplineDistributionPlugin;
use bevy_spline_3d::path_follow::SplineFollowPlugin;

use super::blockout::BlockoutPlugin;
use super::camera::EditorCameraPlugin;
use super::input::EditorInputPlugin;
use super::insert::InsertModePlugin;
use super::marks::CameraMarksPlugin;
use super::scene_loading::SceneLoadingPlugin;
use super::spline_edit::SplineEditPlugin;
use super::state::EditorStatePlugin;
use crate::commands::CommandsPlugin;
use crate::gizmos::EditorGizmosPlugin;
use crate::materials::MaterialsPlugin;
use crate::prefabs::PrefabsPlugin;
use crate::scene::ScenePlugin;
use crate::selection::SelectionPlugin;
use crate::ui::UiPlugin;

/// Configuration for the editor plugin
#[derive(Clone)]
pub struct EditorPluginConfig {
    /// Whether to add the EguiPlugin (disable if your app already has it)
    pub add_egui: bool,
    /// Whether to add physics plugins (disable if your app already has Avian3D)
    pub add_physics: bool,
    /// Whether to pause physics on startup
    pub pause_physics_on_startup: bool,
    /// Whether to add ambient lighting
    pub add_ambient_light: bool,
}

impl Default for EditorPluginConfig {
    fn default() -> Self {
        Self {
            add_egui: true,
            add_physics: true,
            pause_physics_on_startup: false,
            add_ambient_light: true,
        }
    }
}

/// Main editor plugin that bundles all editor functionality
///
/// # Example
///
/// ```no_run
/// use bevy::prelude::*;
/// use bevy_modal_editor::EditorPlugin;
///
/// fn main() {
///     App::new()
///         .add_plugins(DefaultPlugins)
///         .add_plugins(EditorPlugin::default())
///         .run();
/// }
/// ```
///
/// # Configuration
///
/// If your app already has EguiPlugin or Avian3D physics, you can disable them:
///
/// ```no_run
/// use bevy::prelude::*;
/// use bevy_modal_editor::{EditorPlugin, editor::EditorPluginConfig};
///
/// fn main() {
///     App::new()
///         .add_plugins(DefaultPlugins)
///         .add_plugins(EditorPlugin::new(EditorPluginConfig {
///             add_egui: false,  // App already has EguiPlugin
///             add_physics: false,  // App already has Avian3D
///             ..default()
///         }))
///         .run();
/// }
/// ```
pub struct EditorPlugin {
    config: EditorPluginConfig,
}

impl Default for EditorPlugin {
    fn default() -> Self {
        Self {
            config: EditorPluginConfig::default(),
        }
    }
}

impl EditorPlugin {
    /// Create a new editor plugin with custom configuration
    pub fn new(config: EditorPluginConfig) -> Self {
        Self { config }
    }

    /// Create an editor plugin without adding EguiPlugin
    /// (use if your app already has bevy_egui)
    pub fn without_egui() -> Self {
        Self {
            config: EditorPluginConfig {
                add_egui: false,
                ..default()
            },
        }
    }

    /// Create an editor plugin without adding physics plugins
    /// (use if your app already has Avian3D)
    pub fn without_physics() -> Self {
        Self {
            config: EditorPluginConfig {
                add_physics: false,
                ..default()
            },
        }
    }
}

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        // Third-party plugins (conditional)
        // Only add if configured AND not already present
        if self.config.add_egui && !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }
        if self.config.add_physics {
            // Check if physics is already set up by looking for the Time<Physics> resource
            let has_physics = app.world().contains_resource::<Time<Physics>>();
            if !has_physics {
                app.add_plugins(PhysicsPlugins::default());
            }
            if !app.is_plugin_added::<PhysicsDebugPlugin>() {
                app.add_plugins(PhysicsDebugPlugin);
            }
        }

        app
            // Third-party rendering plugins
            .add_plugins(OutlinePlugin)
            .add_plugins(GridMaterialPlugin)
            // Material system
            .add_plugins(MaterialsPlugin)
            // Editor core
            .add_plugins(EditorStatePlugin)
            .add_plugins(EditorInputPlugin)
            .add_plugins(EditorCameraPlugin)
            .add_plugins(CameraMarksPlugin)
            .add_plugins(InsertModePlugin)
            .add_plugins(BlockoutPlugin)
            .add_plugins(SplineEditPlugin)
            .add_plugins(SceneLoadingPlugin)
            .add_plugins(SplineFollowPlugin)
            .add_plugins(SplineDistributionPlugin)
            // Editor systems
            .add_plugins(SelectionPlugin)
            .add_plugins(EditorGizmosPlugin)
            .add_plugins(ScenePlugin)
            .add_plugins(PrefabsPlugin)
            .add_plugins(CommandsPlugin)
            // UI
            .add_plugins(UiPlugin);

        // Pre-startup systems (run before game Startup systems)
        if self.config.add_ambient_light {
            app.add_systems(PreStartup, setup_editor_scene);
        }
        if self.config.pause_physics_on_startup {
            // Defer physics pause by one frame so Avian3D's spatial query pipeline
            // can initialize with the spawned colliders. If we pause at PreStartup,
            // the physics schedule never steps and SpatialQuery::cast_ray returns
            // no hits (breaking selection).
            app.add_systems(Update, pause_physics_on_startup);
        }
    }
}

/// Setup initial editor scene with lighting
fn setup_editor_scene(mut commands: Commands) {
    // Ambient light (now a component in Bevy 0.18+)
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        affects_lightmapped_meshes: true,
    });
}

/// Pause physics simulation after Avian3D has had time to initialize.
///
/// Deferred by a few frames so Avian3D's broad-phase and spatial query pipeline
/// can process the initially-spawned colliders before we freeze physics.
/// If we pause too early, `SpatialQuery::cast_ray` returns no hits (breaking selection).
fn pause_physics_on_startup(
    mut physics_time: ResMut<Time<Physics>>,
    mut frames: Local<u32>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    *frames += 1;
    // Wait a few frames for FixedUpdate to tick and Avian3D to sync colliders
    if *frames >= 3 {
        physics_time.set_relative_speed(0.0);
        *done = true;
        info!("Physics simulation: PAUSED (default)");
    }
}
