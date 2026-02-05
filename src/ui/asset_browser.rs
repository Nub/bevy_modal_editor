//! Asset browser palette — replaces the native file dialog with a fuzzy search
//! palette that recursively scans the `assets/` directory.

use std::path::Path;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass, EguiTextureHandle};

use crate::editor::{EditorMode, EditorState, InsertObjectType, InsertState, StartInsertEvent};
use crate::scene::{LoadSceneEvent, SaveSceneEvent};
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, fuzzy_filter, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use crate::ui::theme::colors;

// ── Types re-exported from the old file_dialog module ────────────────

/// Which texture slot is being picked for
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextureSlot {
    BaseColor,
    NormalMap,
    MetallicRoughness,
    Emissive,
    Occlusion,
}

/// Result of a texture pick operation, consumed by the material editor
#[derive(Resource, Default)]
pub struct TexturePickResult(pub Option<TexturePickData>);

/// Data for a completed texture pick
pub struct TexturePickData {
    pub slot: TextureSlot,
    pub entity: Option<Entity>,
    pub path: String,
}

// ── Browse operation ─────────────────────────────────────────────────

/// What kind of file operation the asset browser is performing.
#[derive(Clone)]
enum BrowseOperation {
    LoadScene,
    SaveScene,
    InsertGltf,
    InsertScene,
    PickTexture { slot: TextureSlot, entity: Option<Entity> },
}

// ── Asset file item ──────────────────────────────────────────────────

/// A single file entry found in the `assets/` directory.
struct AssetFileItem {
    /// Path relative to `assets/` (e.g. "textures/brick.png")
    relative_path: String,
    /// Filename only (e.g. "brick.png")
    filename: String,
    /// Parent directory relative to `assets/`, or empty for root files
    directory: String,
    /// True only for the virtual "Save as" item
    is_save_as: bool,
}

impl PaletteItem for AssetFileItem {
    fn label(&self) -> &str {
        &self.filename
    }

    fn category(&self) -> Option<&str> {
        if self.directory.is_empty() {
            None
        } else {
            Some(&self.directory)
        }
    }

    fn keywords(&self) -> &[String] {
        &[]
    }
}

// ── Scanning ─────────────────────────────────────────────────────────

/// Recursively scan `assets/` for files matching the given extensions.
fn scan_assets(extensions: &[&str]) -> Vec<AssetFileItem> {
    let assets_dir = Path::new("assets");
    if !assets_dir.is_dir() {
        return Vec::new();
    }

    let mut items = Vec::new();
    scan_dir_recursive(assets_dir, assets_dir, extensions, &mut items);
    items.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    items
}

fn scan_dir_recursive(
    base: &Path,
    dir: &Path,
    extensions: &[&str],
    out: &mut Vec<AssetFileItem>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(base, &path, extensions, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            if extensions.iter().any(|e| *e == ext_lower) {
                let relative = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let filename = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let directory = path
                    .parent()
                    .and_then(|p| p.strip_prefix(base).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                out.push(AssetFileItem {
                    relative_path: relative,
                    filename,
                    directory,
                    is_save_as: false,
                });
            }
        }
    }
}

// ── State ────────────────────────────────────────────────────────────

/// Resource managing the asset browser palette state.
#[derive(Resource)]
pub struct AssetBrowserState {
    pub open: bool,
    palette_state: PaletteState,
    operation: Option<BrowseOperation>,
    items: Vec<AssetFileItem>,
    /// Path of the currently previewed texture (relative to assets/).
    preview_path: Option<String>,
    /// Strong handle keeping the preview image alive.
    preview_handle: Option<Handle<Image>>,
    /// Egui texture id for the preview image.
    preview_texture_id: Option<egui::TextureId>,
}

impl Default for AssetBrowserState {
    fn default() -> Self {
        Self {
            open: false,
            palette_state: PaletteState::default(),
            operation: None,
            items: Vec::new(),
            preview_path: None,
            preview_handle: None,
            preview_texture_id: None,
        }
    }
}

impl AssetBrowserState {
    fn open_with(&mut self, operation: BrowseOperation, extensions: &[&str]) {
        self.items = scan_assets(extensions);
        self.palette_state.reset();
        self.operation = Some(operation);
        self.open = true;
        self.preview_path = None;
        self.preview_handle = None;
        self.preview_texture_id = None;
    }

    pub fn open_load_scene(&mut self) {
        self.open_with(BrowseOperation::LoadScene, &["ron"]);
    }

    pub fn open_save_scene(&mut self, current_path: Option<&str>) {
        self.open_with(BrowseOperation::SaveScene, &["ron"]);

        // Prepend virtual "Save as" item
        self.items.insert(
            0,
            AssetFileItem {
                relative_path: String::new(),
                filename: "Save as new file...".to_string(),
                directory: String::new(),
                is_save_as: true,
            },
        );

        // Pre-populate the query with the current scene filename (without extension)
        if let Some(path) = current_path {
            let name = Path::new(path)
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            self.palette_state.query = name.to_string();
        }
    }

    pub fn open_insert_gltf(&mut self) {
        self.open_with(BrowseOperation::InsertGltf, &["gltf", "glb"]);
    }

    pub fn open_insert_scene(&mut self) {
        self.open_with(BrowseOperation::InsertScene, &["ron"]);
    }

    pub fn open_pick_texture(&mut self, slot: TextureSlot, entity: Option<Entity>) {
        self.open_with(
            BrowseOperation::PickTexture { slot, entity },
            &["png", "jpg", "jpeg", "hdr", "exr", "tga", "bmp"],
        );
    }
}

/// Remove the preview texture from egui and clear state fields.
fn cleanup_preview(state: &mut AssetBrowserState, contexts: &mut EguiContexts) {
    if let Some(ref handle) = state.preview_handle.take() {
        contexts.remove_image(handle);
    }
    state.preview_texture_id = None;
    state.preview_path = None;
}

// ── System ───────────────────────────────────────────────────────────

fn draw_asset_browser(
    mut contexts: EguiContexts,
    mut state: ResMut<AssetBrowserState>,
    mut load_events: MessageWriter<LoadSceneEvent>,
    mut save_events: MessageWriter<SaveSceneEvent>,
    mut insert_events: MessageWriter<StartInsertEvent>,
    mut insert_state: ResMut<InsertState>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    editor_state: Res<EditorState>,
    mut texture_pick: ResMut<TexturePickResult>,
    asset_server: Res<AssetServer>,
) -> Result {
    if !editor_state.ui_enabled || !state.open {
        return Ok(());
    }

    // Clone the egui context so we can also use contexts.add_image/remove_image later.
    let ctx = contexts.ctx_mut()?.clone();

    let operation = match &state.operation {
        Some(op) => op.clone(),
        None => {
            state.open = false;
            return Ok(());
        }
    };

    let (title, title_color, subtitle, hint, action_label) = match &operation {
        BrowseOperation::LoadScene => (
            "LOAD SCENE",
            colors::ACCENT_BLUE,
            "Open a scene file",
            "Type to search scenes...",
            "load",
        ),
        BrowseOperation::SaveScene => (
            "SAVE SCENE",
            colors::ACCENT_GREEN,
            "Save to file",
            "Type filename or search...",
            "save",
        ),
        BrowseOperation::InsertGltf => (
            "INSERT GLTF",
            colors::ACCENT_ORANGE,
            "Pick a model to insert",
            "Type to search models...",
            "insert",
        ),
        BrowseOperation::InsertScene => (
            "INSERT SCENE",
            colors::ACCENT_ORANGE,
            "Pick a scene to insert",
            "Type to search scenes...",
            "insert",
        ),
        BrowseOperation::PickTexture { .. } => (
            "PICK TEXTURE",
            colors::ACCENT_PURPLE,
            "Select a texture image",
            "Type to search textures...",
            "select",
        ),
    };

    // Split borrow: get &mut to the inner struct so Rust allows field-level borrows
    let inner: &mut AssetBrowserState = &mut state;

    // ── Texture preview tracking (PickTexture mode only) ────────────
    let is_pick_texture = matches!(&operation, BrowseOperation::PickTexture { .. });
    let preview_panel: Option<Box<dyn FnOnce(&mut egui::Ui)>> = if is_pick_texture {
        // Resolve the currently highlighted item's path
        let filtered = fuzzy_filter(&inner.items, &inner.palette_state.query);
        let highlighted_path = filtered
            .get(inner.palette_state.selected_index)
            .map(|fi| fi.item.relative_path.clone());

        // Update preview if highlighted item changed
        if highlighted_path != inner.preview_path {
            // Remove old egui texture
            if let Some(ref old_handle) = inner.preview_handle.take() {
                contexts.remove_image(old_handle);
                inner.preview_texture_id = None;
            }

            if let Some(ref path) = highlighted_path {
                let handle: Handle<Image> = asset_server.load(path.clone());
                let tex_id = contexts.add_image(EguiTextureHandle::Strong(handle.clone()));
                inner.preview_handle = Some(handle);
                inner.preview_texture_id = Some(tex_id);
            }

            inner.preview_path = highlighted_path;
        }

        // Build preview closure if we have a texture to show
        if let (Some(tex_id), Some(path)) = (inner.preview_texture_id, &inner.preview_path) {
            let tex_id = tex_id;
            let filename = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            Some(Box::new(move |ui: &mut egui::Ui| {
                ui.label(
                    egui::RichText::new("Preview")
                        .small()
                        .strong()
                        .color(colors::TEXT_SECONDARY),
                );
                ui.add_space(4.0);
                let size = ui.available_width().min(220.0);
                ui.image(egui::load::SizedTexture::new(tex_id, [size, size]));
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(&filename)
                        .color(colors::TEXT_PRIMARY)
                        .strong(),
                );
            }))
        } else {
            None
        }
    } else {
        None
    };

    let config = PaletteConfig {
        title,
        title_color,
        subtitle,
        hint_text: hint,
        action_label,
        size: [450.0, 350.0],
        show_categories: true,
        preview_panel,
        ..Default::default()
    };

    let result = draw_fuzzy_palette(&ctx, &mut inner.palette_state, &inner.items, config);

    match result {
        PaletteResult::Selected(index) => {
            let relative_path = inner.items[index].relative_path.clone();
            let is_save_as = inner.items[index].is_save_as;
            let query = inner.palette_state.query.trim().to_string();

            match &operation {
                BrowseOperation::LoadScene => {
                    let full_path = format!("assets/{}", relative_path);
                    load_events.write(LoadSceneEvent { path: full_path });
                }
                BrowseOperation::SaveScene => {
                    let path = if is_save_as {
                        let name = if query.is_empty() {
                            "scene".to_string()
                        } else if query.ends_with(".ron") {
                            query[..query.len() - 4].to_string()
                        } else {
                            query
                        };
                        format!("assets/{}.ron", name)
                    } else {
                        format!("assets/{}", relative_path)
                    };
                    save_events.write(SaveSceneEvent { path });
                }
                BrowseOperation::InsertGltf => {
                    insert_state.gltf_path = Some(relative_path);
                    insert_events.write(StartInsertEvent {
                        object_type: InsertObjectType::Gltf,
                    });
                    next_mode.set(EditorMode::Insert);
                }
                BrowseOperation::InsertScene => {
                    let full_path = format!("assets/{}", relative_path);
                    insert_state.scene_path = Some(full_path);
                    insert_events.write(StartInsertEvent {
                        object_type: InsertObjectType::Scene,
                    });
                    next_mode.set(EditorMode::Insert);
                }
                BrowseOperation::PickTexture { slot, entity } => {
                    texture_pick.0 = Some(TexturePickData {
                        slot: *slot,
                        entity: *entity,
                        path: relative_path,
                    });
                }
            }

            cleanup_preview(&mut state, &mut contexts);
            state.open = false;
            state.operation = None;
        }
        PaletteResult::Closed => {
            cleanup_preview(&mut state, &mut contexts);
            state.open = false;
            state.operation = None;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

// ── Plugin ───────────────────────────────────────────────────────────

pub struct AssetBrowserPlugin;

impl Plugin for AssetBrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetBrowserState>()
            .init_resource::<TexturePickResult>()
            .add_systems(EguiPrimaryContextPass, draw_asset_browser);
    }
}
