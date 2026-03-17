use bevy::prelude::*;

use crate::scene::{GltfLoaded, GltfSource, SceneEntity, SceneSource, SceneSourceLoaded, SplatLoaded, SplatSource};

/// State machine for tracking scene asset loading progress.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, States)]
pub enum SceneLoadingState {
    /// No scene loaded yet
    #[default]
    Unloaded,
    /// Async assets (GLTF, SceneSource) are still loading
    Loading,
    /// All assets have been loaded
    Ready,
}

/// Resource tracking loading progress of async scene assets.
#[derive(Resource, Default)]
pub struct SceneLoadingProgress {
    /// Total number of GLTF sources that need loading
    pub total_gltf: usize,
    /// Number of GLTF sources that have finished loading
    pub loaded_gltf: usize,
    /// Total number of SceneSource assets that need loading
    pub total_scenes: usize,
    /// Number of SceneSource assets that have finished loading
    pub loaded_scenes: usize,
    /// Total number of splat sources that need loading
    pub total_splats: usize,
    /// Number of splat sources that have finished loading
    pub loaded_splats: usize,
}

impl SceneLoadingProgress {
    /// Whether all async assets have been loaded
    pub fn is_complete(&self) -> bool {
        self.loaded_gltf >= self.total_gltf
            && self.loaded_scenes >= self.total_scenes
            && self.loaded_splats >= self.total_splats
    }

    /// Fraction of loading complete (0.0 to 1.0)
    pub fn fraction(&self) -> f32 {
        let total = self.total_gltf + self.total_scenes + self.total_splats;
        if total == 0 {
            return 1.0;
        }
        let loaded = self.loaded_gltf + self.loaded_scenes + self.loaded_splats;
        loaded as f32 / total as f32
    }
}

pub struct SceneLoadingPlugin;

impl Plugin for SceneLoadingPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<SceneLoadingState>()
            .init_resource::<SceneLoadingProgress>()
            .add_systems(
                Update,
                track_loading_progress.run_if(in_state(SceneLoadingState::Loading)),
            )
            .add_systems(Update, detect_scene_load_start);
    }
}

/// Detect when new async assets appear and transition to Loading state.
fn detect_scene_load_start(
    scene_loading_state: Res<State<SceneLoadingState>>,
    mut next_state: ResMut<NextState<SceneLoadingState>>,
    mut progress: ResMut<SceneLoadingProgress>,
    gltf_sources: Query<(Entity, &GltfSource), With<SceneEntity>>,
    gltf_loaded: Query<Entity, (With<GltfSource>, With<GltfLoaded>)>,
    scene_sources: Query<(Entity, &SceneSource), With<SceneEntity>>,
    scene_loaded: Query<Entity, (With<SceneSource>, With<SceneSourceLoaded>)>,
    splat_sources: Query<(Entity, &SplatSource), With<SceneEntity>>,
    splat_loaded: Query<Entity, (With<SplatSource>, With<SplatLoaded>)>,
) {
    // Only check from Unloaded or Ready state
    if *scene_loading_state.get() == SceneLoadingState::Loading {
        return;
    }

    let total_gltf = gltf_sources.iter().count();
    let loaded_gltf = gltf_loaded.iter().count();
    let total_scenes = scene_sources.iter().count();
    let loaded_scenes = scene_loaded.iter().count();
    let total_splats = splat_sources.iter().count();
    let loaded_splats = splat_loaded.iter().count();

    // Check if there are unloaded assets
    let has_pending = (loaded_gltf < total_gltf)
        || (loaded_scenes < total_scenes)
        || (loaded_splats < total_splats);

    if has_pending {
        progress.total_gltf = total_gltf;
        progress.loaded_gltf = loaded_gltf;
        progress.total_scenes = total_scenes;
        progress.loaded_scenes = loaded_scenes;
        progress.total_splats = total_splats;
        progress.loaded_splats = loaded_splats;
        next_state.set(SceneLoadingState::Loading);
        info!(
            "Scene loading started: {}/{} GLTF, {}/{} scenes, {}/{} splats",
            loaded_gltf, total_gltf, loaded_scenes, total_scenes, loaded_splats, total_splats
        );
    } else if total_gltf > 0 || total_scenes > 0 || total_splats > 0 {
        // All loaded, transition to Ready if we have any assets
        if *scene_loading_state.get() == SceneLoadingState::Unloaded {
            next_state.set(SceneLoadingState::Ready);
        }
    }
}

/// Track loading progress and transition to Ready when complete.
fn track_loading_progress(
    mut next_state: ResMut<NextState<SceneLoadingState>>,
    mut progress: ResMut<SceneLoadingProgress>,
    gltf_sources: Query<(Entity, &GltfSource), With<SceneEntity>>,
    gltf_loaded: Query<Entity, (With<GltfSource>, With<GltfLoaded>)>,
    scene_sources: Query<(Entity, &SceneSource), With<SceneEntity>>,
    scene_loaded: Query<Entity, (With<SceneSource>, With<SceneSourceLoaded>)>,
    splat_sources: Query<(Entity, &SplatSource), With<SceneEntity>>,
    splat_loaded: Query<Entity, (With<SplatSource>, With<SplatLoaded>)>,
) {
    progress.total_gltf = gltf_sources.iter().count();
    progress.loaded_gltf = gltf_loaded.iter().count();
    progress.total_scenes = scene_sources.iter().count();
    progress.loaded_scenes = scene_loaded.iter().count();
    progress.total_splats = splat_sources.iter().count();
    progress.loaded_splats = splat_loaded.iter().count();

    if progress.is_complete() {
        next_state.set(SceneLoadingState::Ready);
        info!("Scene loading complete");
    }
}
