mod assets;
mod create;
mod editing;
mod prefab;
mod registry;
mod spawn;

pub use assets::{
    bundle_assets_into_prefab, collect_asset_paths, remap_material_library,
    resolve_prefab_asset_path,
};
pub use create::handle_create_prefab;
pub use editing::{PrefabEditingContext, PrefabOpenConfirmDialog};
pub use prefab::{PrefabInstance, PrefabRoot};
pub use registry::{PrefabEntry, PrefabRegistry};
pub use spawn::{
    handle_spawn_prefab, ClosePrefabEvent, CreatePrefabEvent, OpenPrefabEvent, SpawnPrefabEvent,
};

use bevy::prelude::*;
use bevy_editor_game::SpawnPrefabRequest;
use bevy_egui::EguiPrimaryContextPass;

pub struct PrefabsPlugin;

impl Plugin for PrefabsPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<PrefabInstance>()
            .register_type::<PrefabRoot>()
            .init_resource::<PrefabRegistry>()
            .init_resource::<PrefabOpenConfirmDialog>()
            .add_message::<SpawnPrefabEvent>()
            .add_message::<CreatePrefabEvent>()
            .add_message::<OpenPrefabEvent>()
            .add_message::<ClosePrefabEvent>()
            .add_message::<SpawnPrefabRequest>()
            .add_systems(PreStartup, registry::scan_prefab_directory)
            .add_systems(
                Update,
                (
                    handle_spawn_prefab,
                    handle_create_prefab,
                    editing::handle_open_prefab,
                    editing::handle_close_prefab,
                    editing::check_open_after_save,
                    forward_game_prefab_requests,
                ),
            )
            .add_systems(
                EguiPrimaryContextPass,
                editing::draw_prefab_open_confirm_dialog,
            );
    }
}

/// Forward game-originated prefab spawn requests to the editor's prefab system.
/// Tags spawned entities with `GameEntity` for auto-cleanup on reset.
fn forward_game_prefab_requests(
    mut requests: MessageReader<SpawnPrefabRequest>,
    mut spawn_events: MessageWriter<SpawnPrefabEvent>,
) {
    for request in requests.read() {
        spawn_events.write(SpawnPrefabEvent {
            prefab_name: request.prefab_name.clone(),
            position: request.position,
            rotation: request.rotation,
        });
    }
}
