//! Particle preset palette — browse and apply/insert particle effect presets.

use bevy::prelude::*;
use bevy_egui::egui;

use bevy_vfx::{VfxLibrary, VfxSystem};
use crate::scene::SceneEntity;
use crate::selection::Selected;
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use crate::ui::theme::colors;

use super::CommandPaletteState;

/// A library preset entry for fuzzy filtering.
struct PresetItem {
    name: String,
    is_new_preset: bool,
}

impl PaletteItem for PresetItem {
    fn label(&self) -> &str {
        &self.name
    }

    fn always_visible(&self) -> bool {
        self.is_new_preset
    }

    fn accent_color(&self) -> Option<egui::Color32> {
        if self.is_new_preset {
            Some(colors::ACCENT_GREEN)
        } else {
            None
        }
    }
}

/// Draw the particle preset palette using the shared fuzzy palette widget.
pub(super) fn draw_particle_preset_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    library: &Res<VfxLibrary>,
    selected_particle: Option<Entity>,
    commands: &mut Commands,
) -> Result {
    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState::from_bridge(
        std::mem::take(&mut state.query),
        state.selected_index,
        state.just_opened,
    );

    // Build items: "New Preset" (always-visible) at index 0, then sorted library presets
    let new_label = if palette_state.query.trim().is_empty() {
        "+ New Preset".to_string()
    } else {
        format!("+ New Preset \"{}\"", palette_state.query.trim())
    };
    let mut items: Vec<PresetItem> = vec![PresetItem {
        name: new_label,
        is_new_preset: true,
    }];

    let mut names: Vec<String> = library.effects.keys().cloned().collect();
    names.sort();
    items.extend(names.iter().map(|n| PresetItem {
        name: n.clone(),
        is_new_preset: false,
    }));

    let config = PaletteConfig {
        title: "PARTICLES",
        title_color: colors::ACCENT_ORANGE,
        subtitle: "Particle library",
        hint_text: "Type to search presets...",
        action_label: "apply",
        size: [340.0, 340.0],
        show_categories: false,
        ..Default::default()
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            if items[index].is_new_preset {
                // "New Preset" — create a default particle effect in the library
                let query_text = state.query.trim().to_string();
                let new_name = new_preset_name(&query_text, library);
                let system = VfxSystem::default();
                let spawn_name = new_name.clone();
                let selected = selected_particle;
                commands.queue(move |world: &mut World| {
                    world
                        .resource_mut::<VfxLibrary>()
                        .effects
                        .insert(new_name.clone(), system.clone());
                    if let Some(entity) = selected {
                        // Apply to selected particle entity
                        if let Ok(mut e) = world.get_entity_mut(entity) {
                            e.insert(system);
                        }
                    } else {
                        // No selection — spawn a new entity with the preset
                        world.spawn((
                            SceneEntity,
                            Name::new(spawn_name),
                            system,
                            Transform::default(),
                            Visibility::default(),
                            avian3d::prelude::Collider::sphere(
                                crate::constants::physics::LIGHT_COLLIDER_RADIUS,
                            ),
                            Selected,
                        ));
                    }
                });
            } else {
                // Existing preset selected
                let preset_name = items[index].name.clone();
                if let Some(system) = library.effects.get(&preset_name) {
                    let system = system.clone();
                    let selected = selected_particle;
                    let name = preset_name.clone();
                    commands.queue(move |world: &mut World| {
                        if let Some(entity) = selected {
                            // Apply preset to selected particle entity
                            if let Ok(mut e) = world.get_entity_mut(entity) {
                                e.insert(system);
                            }
                        } else {
                            // No selection — spawn a new entity with the preset
                            world.spawn((
                                SceneEntity,
                                Name::new(name),
                                system,
                                Transform::default(),
                                Visibility::default(),
                                avian3d::prelude::Collider::sphere(
                                    crate::constants::physics::LIGHT_COLLIDER_RADIUS,
                                ),
                                Selected,
                            ));
                        }
                    });
                }
            }
            state.open = false;
        }
        PaletteResult::Closed => {
            state.open = false;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

/// Generate a unique preset name.
fn new_preset_name(query: &str, library: &VfxLibrary) -> String {
    let base = if query.is_empty() {
        "New Particle".to_string()
    } else {
        query.to_string()
    };

    if !library.effects.contains_key(&base) {
        return base;
    }

    for i in 2.. {
        let candidate = format!("{} {}", base, i);
        if !library.effects.contains_key(&candidate) {
            return candidate;
        }
    }
    base
}
