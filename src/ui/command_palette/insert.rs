//! Insert mode palette with 3D preview panel.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::editor::{EditorMode, InsertObjectType, InsertState, StartInsertEvent};
use bevy_vfx::VfxLibrary;
use crate::prefabs::PrefabRegistry;
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, fuzzy_filter, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use crate::ui::insert_preview::{InsertPreviewKind, InsertPreviewState};
use crate::ui::prefab_preview::PrefabPreviewState;
use crate::ui::theme::colors;

use super::commands::{CommandAction, CommandEvents, CommandRegistry};
use super::CommandPaletteState;

/// Item for the insert object palette
struct InsertItem {
    name: String,
    category: String,
    keywords: Vec<String>,
    action: CommandAction,
}

impl PaletteItem for InsertItem {
    fn label(&self) -> &str {
        &self.name
    }

    fn category(&self) -> Option<&str> {
        Some(&self.category)
    }

    fn keywords(&self) -> &[String] {
        &self.keywords
    }
}

/// Map a `CommandAction` to an `InsertPreviewKind` for the 3D preview.
fn action_to_preview_kind(action: &CommandAction) -> Option<InsertPreviewKind> {
    match action {
        CommandAction::SpawnPrimitive(shape) => Some(InsertPreviewKind::Primitive(*shape)),
        CommandAction::SpawnPointLight => Some(InsertPreviewKind::PointLight),
        CommandAction::SpawnDirectionalLight => Some(InsertPreviewKind::DirectionalLight),
        CommandAction::SpawnGroup => Some(InsertPreviewKind::Group),
        CommandAction::SpawnSpline(_) => Some(InsertPreviewKind::Spline),
        CommandAction::SpawnFogVolume => Some(InsertPreviewKind::FogVolume),
        CommandAction::SpawnStairs => Some(InsertPreviewKind::Stairs),
        CommandAction::SpawnRamp => Some(InsertPreviewKind::Ramp),
        CommandAction::SpawnArch => Some(InsertPreviewKind::Arch),
        CommandAction::SpawnLShape => Some(InsertPreviewKind::LShape),
        CommandAction::SpawnDecal => Some(InsertPreviewKind::Decal),
        CommandAction::SpawnLibraryMesh(_) => None, // No 3D preview for library meshes yet
        CommandAction::SpawnParticleEffect => None, // No 3D preview for particles
        CommandAction::SpawnParticlePreset(_) => None, // No 3D preview for particle presets
        CommandAction::SpawnEffectPreset(_) => None,   // No 3D preview for effect presets
        CommandAction::SpawnPrefab(_) => Some(InsertPreviewKind::Prefab),
        _ => None,
    }
}

/// Draw the insert object palette with a 3D preview panel.
pub(super) fn draw_insert_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    registry: &Res<CommandRegistry>,
    insert_preview_state: &mut ResMut<InsertPreviewState>,
    events: &mut CommandEvents,
    next_mode: &mut ResMut<NextState<EditorMode>>,
    vfx_library: &Res<VfxLibrary>,
    effect_library: &Res<crate::effects::EffectLibrary>,
    insert_state: &mut ResMut<InsertState>,
    prefab_registry: &Res<PrefabRegistry>,
    prefab_preview_state: &mut ResMut<PrefabPreviewState>,
) -> Result {
    // Build insert item list from registry
    let mut items: Vec<InsertItem> = registry
        .commands
        .iter()
        .filter(|cmd| cmd.insertable)
        .map(|cmd| InsertItem {
            name: cmd.name.clone(),
            category: cmd.category.to_string(),
            keywords: cmd.keywords.clone(),
            action: cmd.action.clone(),
        })
        .collect();

    // Add VFX presets as insertable items
    let mut preset_names: Vec<&String> = vfx_library.effects.keys().collect();
    preset_names.sort();
    for name in preset_names {
        items.push(InsertItem {
            name: format!("Particle: {}", name),
            category: "Effects".to_string(),
            keywords: vec![
                "particle".into(),
                "preset".into(),
                "vfx".into(),
                "fx".into(),
                name.to_lowercase(),
            ],
            action: CommandAction::SpawnParticlePreset(name.clone()),
        });
    }

    // Add effect presets as insertable items
    let mut effect_preset_names: Vec<&String> = effect_library.effects.keys().collect();
    effect_preset_names.sort();
    for name in effect_preset_names {
        items.push(InsertItem {
            name: format!("Effect: {}", name),
            category: "Effects".to_string(),
            keywords: vec![
                "effect".into(),
                "preset".into(),
                "sequence".into(),
                "vfx".into(),
                name.to_lowercase(),
            ],
            action: CommandAction::SpawnEffectPreset(name.clone()),
        });
    }

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState::from_bridge(
        std::mem::take(&mut state.query),
        state.selected_index,
        state.just_opened,
    );

    // Determine highlighted item for the preview
    let filtered = fuzzy_filter(&items, &palette_state.query);
    let clamped = if filtered.is_empty() {
        0
    } else {
        palette_state.selected_index.min(filtered.len() - 1)
    };

    // Update the 3D preview to match the highlighted item
    let preview_kind = filtered
        .get(clamped)
        .and_then(|fi| action_to_preview_kind(&fi.item.action));

    // Check if highlighted item is a prefab (uses separate preview system)
    let is_prefab_preview = matches!(preview_kind, Some(InsertPreviewKind::Prefab));
    if is_prefab_preview {
        // Drive the prefab preview system
        let prefab_scene_path = filtered.get(clamped).and_then(|fi| {
            if let CommandAction::SpawnPrefab(name) = &fi.item.action {
                prefab_registry
                    .get(name)
                    .map(|entry| entry.scene_path.to_string_lossy().to_string())
            } else {
                None
            }
        });
        prefab_preview_state.current_path = prefab_scene_path;
        insert_preview_state.current_kind = None; // Clear the other preview
    } else {
        insert_preview_state.current_kind = preview_kind;
        prefab_preview_state.current_path = None; // Clear the prefab preview
    }

    // Capture preview info for the panel closure
    let preview_texture_id = if is_prefab_preview {
        prefab_preview_state.texture.egui_texture_id
    } else {
        insert_preview_state.texture.egui_texture_id
    };
    let preview_name = filtered.get(clamped).map(|fi| fi.item.name.clone());

    let has_preview = is_prefab_preview || insert_preview_state.current_kind.is_some();
    let preview_panel: Option<Box<dyn FnOnce(&mut egui::Ui) + '_>> =
        Some(Box::new(move |ui: &mut egui::Ui| {
            ui.label(
                egui::RichText::new("Preview")
                    .small()
                    .strong()
                    .color(colors::TEXT_SECONDARY),
            );
            ui.add_space(4.0);
            if has_preview {
                if let Some(tex_id) = preview_texture_id {
                    let size = ui.available_width().min(220.0);
                    ui.image(egui::load::SizedTexture::new(tex_id, [size, size]));
                }
            } else {
                let size = ui.available_width().min(220.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "No preview available",
                    egui::FontId::proportional(13.0),
                    colors::TEXT_MUTED,
                );
            }
            if let Some(name) = &preview_name {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(name)
                        .color(colors::TEXT_PRIMARY)
                        .strong(),
                );
            }
        }));

    let config = PaletteConfig {
        title: "INSERT",
        title_color: colors::ACCENT_GREEN,
        subtitle: "Select object, then click to place",
        hint_text: "Type to search objects...",
        action_label: "insert",
        size: [340.0, 340.0],
        show_categories: true,
        preview_panel,
        ..Default::default()
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            let action = &items[index].action;
            match action {
                CommandAction::SpawnPrimitive(shape) => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Primitive(*shape),
                    });
                }
                CommandAction::SpawnPointLight => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::PointLight,
                    });
                }
                CommandAction::SpawnDirectionalLight => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::DirectionalLight,
                    });
                }
                CommandAction::SpawnGroup => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Group,
                    });
                }
                CommandAction::InsertGltf => {
                    state.open_asset_browser_insert_gltf();
                    next_mode.set(EditorMode::View);
                    insert_preview_state.current_kind = None;
                    return Ok(());
                }
                CommandAction::InsertScene => {
                    state.open_asset_browser_insert_scene();
                    next_mode.set(EditorMode::View);
                    insert_preview_state.current_kind = None;
                    return Ok(());
                }
                CommandAction::SpawnSpline(spline_type) => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Spline(*spline_type),
                    });
                }
                CommandAction::SpawnFogVolume => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::FogVolume,
                    });
                }
                CommandAction::SpawnStairs => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Stairs,
                    });
                }
                CommandAction::SpawnRamp => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Ramp,
                    });
                }
                CommandAction::SpawnArch => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Arch,
                    });
                }
                CommandAction::SpawnLShape => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::LShape,
                    });
                }
                CommandAction::SpawnParticleEffect => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::ParticleEffect,
                    });
                }
                CommandAction::SpawnDecal => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Decal,
                    });
                }
                CommandAction::SpawnLibraryMesh(mesh_name) => {
                    events.spawn_entity.write(crate::scene::SpawnEntityEvent {
                        kind: crate::scene::SpawnEntityKind::LibraryMesh(mesh_name.clone()),
                        position: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::SpawnParticlePreset(preset_name) => {
                    // Spawn the particle preset directly (no click-to-place preview)
                    events.spawn_entity.write(crate::scene::SpawnEntityEvent {
                        kind: crate::scene::SpawnEntityKind::ParticlePreset(preset_name.clone()),
                        position: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::SpawnEffectPreset(preset_name) => {
                    events.spawn_entity.write(crate::scene::SpawnEntityEvent {
                        kind: crate::scene::SpawnEntityKind::EffectPreset(preset_name.clone()),
                        position: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::SpawnPrefab(prefab_name) => {
                    insert_state.prefab_name = Some(prefab_name.clone());
                    // Look up the scene path from the registry for preview
                    if let Some(entry) = prefab_registry.get(prefab_name) {
                        insert_state.scene_path = Some(entry.scene_path.to_string_lossy().to_string());
                    }
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Prefab,
                    });
                }
                _ => {}
            }
            state.open = false;
            insert_preview_state.current_kind = None;
            prefab_preview_state.current_path = None;
        }
        PaletteResult::Closed => {
            state.open = false;
            insert_preview_state.current_kind = None;
            prefab_preview_state.current_path = None;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}
