mod museum;

use bevy::prelude::*;
use bevy_egui::egui;
use bevy_egui::EguiPrimaryContextPass;
use bevy_outliner::prelude::{HasSilhouetteMesh, SilhouetteMesh};

use super::{SaveSceneEvent, SceneEntity, SceneFile};
use crate::editor::EditorState;
use crate::ui::theme::{colors, window_frame};

/// A single section of the museum grid (e.g. "Primitives", "Objects/furniture", "Materials").
pub struct MuseumGridSection {
    pub title: String,
    pub rows: usize,
    pub cols: usize,
    pub spacing: f32,
    /// World-space origin of the first cell center in this section.
    pub origin: Vec3,
}

/// Resource storing museum grid parameters for gizmo rendering.
/// Present only while a museum scene is active.
#[derive(Resource, Default)]
pub struct MuseumGrid {
    pub sections: Vec<MuseumGridSection>,
}

/// Which generator to run
#[derive(Clone)]
pub enum SceneGenerator {
    Museum,
}

/// Event requesting scene generation (triggers save prompt if needed)
#[derive(Message)]
pub struct GenerateSceneEvent {
    pub generator: SceneGenerator,
}

/// Resource tracking the pending generation while save dialog is open
#[derive(Resource, Default)]
pub struct GeneratorState {
    pending: Option<SceneGenerator>,
    dialog_open: bool,
    /// When true, run the generator after the next save completes
    run_after_save: bool,
}

pub struct SceneGeneratorPlugin;

impl Plugin for SceneGeneratorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GeneratorState>()
            .add_message::<GenerateSceneEvent>()
            .add_systems(
                Update,
                (
                    handle_generate_scene,
                    check_post_save_generation,
                    draw_museum_grid_gizmos,
                ),
            )
            .add_systems(EguiPrimaryContextPass, draw_generator_save_dialog);
    }
}

/// Handle incoming generation requests
fn handle_generate_scene(
    mut events: MessageReader<GenerateSceneEvent>,
    scene_file: Res<SceneFile>,
    mut gen_state: ResMut<GeneratorState>,
    mut commands: Commands,
) {
    for event in events.read() {
        if scene_file.modified {
            gen_state.pending = Some(event.generator.clone());
            gen_state.dialog_open = true;
        } else {
            let generator = event.generator.clone();
            commands.queue(RunGeneratorCommand { generator });
        }
    }
}

/// Check if a save just completed and we need to run the generator
fn check_post_save_generation(
    scene_file: Res<SceneFile>,
    mut gen_state: ResMut<GeneratorState>,
    mut commands: Commands,
) {
    if gen_state.run_after_save && !scene_file.modified {
        if let Some(generator) = gen_state.pending.take() {
            gen_state.run_after_save = false;
            commands.queue(RunGeneratorCommand { generator });
        }
    }
}

enum SaveDialogAction {
    None,
    Save,
    Discard,
    Cancel,
}

/// Draw the save/discard/cancel dialog when generating with unsaved changes
fn draw_generator_save_dialog(world: &mut World) {
    let dialog_open = world
        .get_resource::<GeneratorState>()
        .is_some_and(|s| s.dialog_open);
    if !dialog_open {
        return;
    }

    let ui_enabled = world
        .get_resource::<EditorState>()
        .is_some_and(|s| s.ui_enabled);
    if !ui_enabled {
        return;
    }

    let ctx = {
        let Some(mut egui_ctx) = world
            .query::<&mut bevy_egui::EguiContext>()
            .iter_mut(world)
            .next()
        else {
            return;
        };
        egui_ctx.get_mut().clone()
    };

    let mut action = SaveDialogAction::None;

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = SaveDialogAction::Cancel;
    }

    egui::Window::new("Unsaved Changes")
        .collapsible(false)
        .resizable(false)
        .frame(window_frame(&ctx.style()))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([360.0, 120.0])
        .show(&ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Scene has unsaved changes. Save before generating?")
                        .color(colors::TEXT_PRIMARY),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(60.0);
                    if ui.button("Save").clicked() {
                        action = SaveDialogAction::Save;
                    }
                    ui.add_space(8.0);
                    if ui
                        .button(egui::RichText::new("Discard").color(colors::STATUS_WARNING))
                        .clicked()
                    {
                        action = SaveDialogAction::Discard;
                    }
                    ui.add_space(8.0);
                    if ui.button("Cancel").clicked() {
                        action = SaveDialogAction::Cancel;
                    }
                });
            });
        });

    match action {
        SaveDialogAction::Save => {
            world.resource_mut::<GeneratorState>().dialog_open = false;

            let path = world.resource::<SceneFile>().path.clone();
            if let Some(path) = path {
                // Save then run generator after save completes
                world.resource_mut::<GeneratorState>().run_after_save = true;
                world.write_message(SaveSceneEvent { path });
            } else {
                // No save path — run immediately (can't save untitled without a path)
                let generator = world.resource_mut::<GeneratorState>().pending.take();
                if let Some(generator) = generator {
                    run_generator(world, &generator);
                }
            }
        }
        SaveDialogAction::Discard => {
            world.resource_mut::<GeneratorState>().dialog_open = false;
            world.resource_mut::<GeneratorState>().run_after_save = false;
            let generator = world.resource_mut::<GeneratorState>().pending.take();
            if let Some(generator) = generator {
                run_generator(world, &generator);
            }
        }
        SaveDialogAction::Cancel => {
            world.resource_mut::<GeneratorState>().dialog_open = false;
            world.resource_mut::<GeneratorState>().run_after_save = false;
            world.resource_mut::<GeneratorState>().pending = None;
        }
        SaveDialogAction::None => {}
    }
}

/// Dispatch to the appropriate generator function
fn run_generator(world: &mut World, generator: &SceneGenerator) {
    clear_scene(world);
    match generator {
        SceneGenerator::Museum => museum::generate_museum(world),
    }
    // Reset scene file to untitled
    let mut scene_file = world.resource_mut::<SceneFile>();
    scene_file.path = None;
    scene_file.modified = false;
}

/// Despawn all SceneEntity entities and their silhouettes
fn clear_scene(world: &mut World) {
    world.remove_resource::<MuseumGrid>();
    // Clean up silhouettes first (same pattern as restore_scene_from_data)
    let silhouettes: Vec<Entity> = {
        let mut query = world.query_filtered::<&HasSilhouetteMesh, With<SceneEntity>>();
        query.iter(world).map(|h| h.silhouette).collect()
    };
    let child_silhouettes: Vec<Entity> = {
        let mut query = world.query_filtered::<&HasSilhouetteMesh, Without<SceneEntity>>();
        query.iter(world).map(|h| h.silhouette).collect()
    };
    for entity in silhouettes.into_iter().chain(child_silhouettes) {
        world.despawn(entity);
    }
    let orphaned: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SilhouetteMesh>>();
        query.iter(world).collect()
    };
    for entity in orphaned {
        world.despawn(entity);
    }

    // Despawn scene entities
    let entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
        query.iter(world).collect()
    };
    for entity in entities {
        world.despawn(entity);
    }
}

/// Deferred command to run a generator
struct RunGeneratorCommand {
    generator: SceneGenerator,
}

impl bevy::prelude::Command for RunGeneratorCommand {
    fn apply(self, world: &mut World) {
        run_generator(world, &self.generator);
    }
}

/// Scale for gizmo text characters (world units per glyph unit).
const LABEL_SCALE: f32 = 0.08;
/// Gap between characters in world units.
const LABEL_GAP: f32 = 0.04;
/// Inset from cell corner.
const LABEL_MARGIN: f32 = 0.25;
/// Glyph width in glyph-space units.
const GLYPH_W: f32 = 3.0;

/// Draw grid lines, section titles, and coordinate labels for the museum layout.
fn draw_museum_grid_gizmos(
    mut gizmos: Gizmos,
    grid: Option<Res<MuseumGrid>>,
    editor_state: Res<EditorState>,
) {
    let Some(grid) = grid else { return };
    if !editor_state.gizmos_visible {
        return;
    }

    let line_color = Color::srgba(0.6, 0.8, 1.0, 0.7);
    let label_color = Color::srgba(0.7, 0.85, 1.0, 0.85);
    let title_color = Color::srgba(0.9, 0.95, 1.0, 0.95);
    let y = 0.01;

    for section in &grid.sections {
        let ox = section.origin.x;
        let oz = section.origin.z;
        let half = section.spacing * 0.5;

        let x_min = ox - half;
        let x_max = ox + (section.cols as f32 - 1.0) * section.spacing + half;
        let z_min = oz - half;
        let z_max = oz + (section.rows as f32 - 1.0) * section.spacing + half;

        // Grid lines
        for row in 0..=section.rows {
            let z = oz + row as f32 * section.spacing - half;
            gizmos.line(Vec3::new(x_min, y, z), Vec3::new(x_max, y, z), line_color);
        }
        for col in 0..=section.cols {
            let x = ox + col as f32 * section.spacing - half;
            gizmos.line(Vec3::new(x, y, z_min), Vec3::new(x, y, z_max), line_color);
        }

        // Section title above the grid (toward -Z from first row)
        let title_x = ox - half + LABEL_MARGIN;
        let title_z = oz - half - 0.3;
        let title_upper = section.title.to_uppercase();
        draw_gizmo_text(&mut gizmos, &title_upper, title_x, title_z, title_color);

        // Cell coordinate labels
        for row in 0..section.rows {
            for col in 0..section.cols {
                let label = format!("{}{:03}", (b'A' + row as u8) as char, col + 1);
                let base_x = ox + col as f32 * section.spacing - half + LABEL_MARGIN;
                let base_z = oz + row as f32 * section.spacing + half - LABEL_MARGIN;
                draw_gizmo_text(&mut gizmos, &label, base_x, base_z, label_color);
            }
        }
    }
}

/// Draw a string on the floor plane using gizmo line segments.
/// Text reads along +X, with character tops toward -Z.
fn draw_gizmo_text(gizmos: &mut Gizmos, text: &str, mut x: f32, z: f32, color: Color) {
    let y = 0.02;
    let advance = GLYPH_W * LABEL_SCALE + LABEL_GAP;

    for ch in text.chars() {
        for &(x1, y1, x2, y2) in glyph_segments(ch) {
            gizmos.line(
                Vec3::new(x + x1 * LABEL_SCALE, y, z - y1 * LABEL_SCALE),
                Vec3::new(x + x2 * LABEL_SCALE, y, z - y2 * LABEL_SCALE),
                color,
            );
        }
        x += advance;
    }
}

/// Line segments for a character in a 3-wide, 5-tall glyph space.
/// Returns `(x1, y1, x2, y2)` pairs.
fn glyph_segments(ch: char) -> &'static [(f32, f32, f32, f32)] {
    match ch {
        '0' => &[
            (0., 0., 3., 0.),
            (3., 0., 3., 5.),
            (3., 5., 0., 5.),
            (0., 5., 0., 0.),
        ],
        '1' => &[(1.5, 0., 1.5, 5.), (0.5, 4., 1.5, 5.), (0., 0., 3., 0.)],
        '2' => &[
            (0., 5., 3., 5.),
            (3., 5., 3., 2.5),
            (3., 2.5, 0., 2.5),
            (0., 2.5, 0., 0.),
            (0., 0., 3., 0.),
        ],
        '3' => &[
            (0., 5., 3., 5.),
            (3., 5., 3., 0.),
            (3., 0., 0., 0.),
            (0., 2.5, 3., 2.5),
        ],
        '4' => &[
            (0., 5., 0., 2.5),
            (0., 2.5, 3., 2.5),
            (3., 5., 3., 0.),
        ],
        '5' => &[
            (3., 5., 0., 5.),
            (0., 5., 0., 2.5),
            (0., 2.5, 3., 2.5),
            (3., 2.5, 3., 0.),
            (3., 0., 0., 0.),
        ],
        '6' => &[
            (3., 5., 0., 5.),
            (0., 5., 0., 0.),
            (0., 0., 3., 0.),
            (3., 0., 3., 2.5),
            (3., 2.5, 0., 2.5),
        ],
        '7' => &[(0., 5., 3., 5.), (3., 5., 1., 0.)],
        '8' => &[
            (0., 0., 3., 0.),
            (3., 0., 3., 5.),
            (3., 5., 0., 5.),
            (0., 5., 0., 0.),
            (0., 2.5, 3., 2.5),
        ],
        '9' => &[
            (3., 2.5, 0., 2.5),
            (0., 2.5, 0., 5.),
            (0., 5., 3., 5.),
            (3., 5., 3., 0.),
            (3., 0., 0., 0.),
        ],
        'A' => &[
            (0., 0., 0., 5.),
            (0., 5., 3., 5.),
            (3., 5., 3., 0.),
            (0., 2.5, 3., 2.5),
        ],
        'B' => &[
            (0., 0., 0., 5.),
            (0., 5., 2., 5.),
            (2., 5., 3., 4.),
            (3., 4., 2., 2.5),
            (2., 2.5, 0., 2.5),
            (2., 2.5, 3., 1.5),
            (3., 1.5, 2., 0.),
            (2., 0., 0., 0.),
        ],
        'C' => &[(3., 5., 0., 5.), (0., 5., 0., 0.), (0., 0., 3., 0.)],
        'D' => &[
            (0., 0., 0., 5.),
            (0., 5., 2., 5.),
            (2., 5., 3., 2.5),
            (3., 2.5, 2., 0.),
            (2., 0., 0., 0.),
        ],
        'E' => &[
            (3., 5., 0., 5.),
            (0., 5., 0., 0.),
            (0., 0., 3., 0.),
            (0., 2.5, 2., 2.5),
        ],
        'F' => &[
            (3., 5., 0., 5.),
            (0., 5., 0., 0.),
            (0., 2.5, 2., 2.5),
        ],
        'G' => &[
            (3., 5., 0., 5.),
            (0., 5., 0., 0.),
            (0., 0., 3., 0.),
            (3., 0., 3., 2.5),
            (3., 2.5, 1.5, 2.5),
        ],
        'H' => &[
            (0., 0., 0., 5.),
            (3., 0., 3., 5.),
            (0., 2.5, 3., 2.5),
        ],
        'I' => &[(0., 5., 3., 5.), (1.5, 5., 1.5, 0.), (0., 0., 3., 0.)],
        'J' => &[
            (0., 5., 3., 5.),
            (3., 5., 3., 0.),
            (3., 0., 0., 0.),
            (0., 0., 0., 1.5),
        ],
        'K' => &[
            (0., 0., 0., 5.),
            (0., 2.5, 3., 5.),
            (0., 2.5, 3., 0.),
        ],
        'L' => &[(0., 5., 0., 0.), (0., 0., 3., 0.)],
        'M' => &[
            (0., 0., 0., 5.),
            (0., 5., 1.5, 2.5),
            (1.5, 2.5, 3., 5.),
            (3., 5., 3., 0.),
        ],
        'N' => &[(0., 0., 0., 5.), (0., 5., 3., 0.), (3., 0., 3., 5.)],
        'O' => &[
            (0., 0., 3., 0.),
            (3., 0., 3., 5.),
            (3., 5., 0., 5.),
            (0., 5., 0., 0.),
        ],
        'P' => &[
            (0., 0., 0., 5.),
            (0., 5., 3., 5.),
            (3., 5., 3., 2.5),
            (3., 2.5, 0., 2.5),
        ],
        'R' => &[
            (0., 0., 0., 5.),
            (0., 5., 3., 5.),
            (3., 5., 3., 2.5),
            (3., 2.5, 0., 2.5),
            (1.5, 2.5, 3., 0.),
        ],
        'S' => &[
            (3., 5., 0., 5.),
            (0., 5., 0., 2.5),
            (0., 2.5, 3., 2.5),
            (3., 2.5, 3., 0.),
            (3., 0., 0., 0.),
        ],
        'T' => &[(0., 5., 3., 5.), (1.5, 5., 1.5, 0.)],
        'U' => &[(0., 5., 0., 0.), (0., 0., 3., 0.), (3., 0., 3., 5.)],
        'V' => &[(0., 5., 1.5, 0.), (1.5, 0., 3., 5.)],
        'W' => &[
            (0., 5., 0., 0.),
            (0., 0., 1.5, 2.5),
            (1.5, 2.5, 3., 0.),
            (3., 0., 3., 5.),
        ],
        'X' => &[(0., 0., 3., 5.), (0., 5., 3., 0.)],
        'Y' => &[
            (0., 5., 1.5, 2.5),
            (3., 5., 1.5, 2.5),
            (1.5, 2.5, 1.5, 0.),
        ],
        'Z' => &[(0., 5., 3., 5.), (3., 5., 0., 0.), (0., 0., 3., 0.)],
        _ => &[],
    }
}
