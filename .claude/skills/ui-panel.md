# UI Panel Scaffolding

Use this skill when the user wants to create a new UI panel, window, popup, dialog, or overlay for the editor. Examples: "add a performance panel", "add a properties popup", "create a debug overlay", "add a search dialog".

## Architecture

All UI is built with **bevy_egui**. Each panel is a self-contained Bevy plugin with:
- A state `Resource` (if the panel has state)
- A draw system running in `EguiPrimaryContextPass`
- Theme integration via `src/ui/theme.rs`

Panels are registered in `UiPlugin` (`src/ui/mod.rs`), organized into:
- **Main panels** — persistent docked areas (hierarchy, inspector, toolbar)
- **Popups and dialogs** — modal overlays (command palette, file dialog, search)

## File Locations

- **New panel module**: `src/ui/my_panel.rs` (create new file)
- **Registration**: `src/ui/mod.rs` — add `mod`, `pub use`, and plugin to `UiPlugin`
- **Theme**: `src/ui/theme.rs` — colors, frames, dialog helpers, panel constants
- **Fuzzy search**: `src/ui/fuzzy_palette.rs` — reusable searchable list widget (used by `command_palette/` submodules)
- **Input guard**: `src/utils.rs` — `should_process_input()`

## Step-by-Step

### 1. Create the Panel Module

Create `src/ui/my_panel.rs`. Choose the appropriate template below.

### 2. Register in `src/ui/mod.rs`

```rust
// Add module declaration (at top with other mods)
mod my_panel;

// Add public re-export (with other pub use statements)
pub use my_panel::*;

// Add plugin to UiPlugin::build() in the appropriate section:
impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // ...
        // Main panels - persistent UI areas
        .add_plugins((
            // ... existing panels ...
            MyPanelPlugin,              // <-- for persistent panels
        ))
        // Popups and dialogs - modal UI elements
        .add_plugins((
            // ... existing popups ...
            MyDialogPlugin,             // <-- for popups/dialogs
        ));
    }
}
```

## Templates

### Template A: Docked Panel (like Inspector, Hierarchy)

Use for persistent panels that dock to an edge of the viewport.

```rust
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState};
use crate::ui::theme::{colors, panel, panel_frame};

#[derive(Resource, Default)]
pub struct MyPanelState {
    // Panel-specific state fields
}

pub struct MyPanelPlugin;

impl Plugin for MyPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MyPanelState>()
            .add_systems(EguiPrimaryContextPass, draw_my_panel);
    }
}

fn draw_my_panel(
    mut contexts: EguiContexts,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
    mut state: ResMut<MyPanelState>,
    // Add queries and resources as needed
) -> Result {
    if !editor_state.ui_enabled {
        return Ok(());
    }

    // Optional: only show in specific modes
    // if *mode.get() != EditorMode::View { return Ok(()); }

    let ctx = contexts.ctx_mut()?;

    // Calculate available height (accounts for status bar)
    let available_height = ctx.content_rect().height()
        - panel::STATUS_BAR_HEIGHT
        - panel::WINDOW_PADDING * 2.0;

    egui::Window::new("My Panel")
        .default_size([panel::DEFAULT_WIDTH, available_height])
        .min_width(panel::MIN_WIDTH)
        // Anchor: RIGHT_TOP for right side, LEFT_TOP for left side
        .anchor(
            egui::Align2::RIGHT_TOP,
            [-panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
        .frame(panel_frame(&ctx.style()))
        .show(ctx, |ui| {
            // Panel content
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Section Title")
                        .strong()
                        .color(colors::TEXT_PRIMARY),
                );
                ui.separator();

                // Content here
            });
        });

    Ok(())
}
```

### Template B: Floating Window (like Edit Info)

Use for small, non-modal info windows or toolbars.

```rust
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState};
use crate::ui::theme::colors;

pub struct MyWindowPlugin;

impl Plugin for MyWindowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_my_window);
    }
}

fn draw_my_window(
    mut contexts: EguiContexts,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
) -> Result {
    if !editor_state.ui_enabled {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    egui::Window::new("Info")
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::LEFT_BOTTOM, [210.0, -35.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Label").color(colors::TEXT_MUTED));
                ui.label(egui::RichText::new("Value").strong().color(colors::ACCENT_BLUE));
            });
        });

    Ok(())
}
```

### Template C: Modal Dialog (centered popup)

Use for confirmation dialogs, input prompts, or settings.

```rust
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::EditorState;
use crate::ui::theme::{colors, draw_centered_dialog, DialogResult};

#[derive(Resource, Default)]
pub struct MyDialogState {
    pub open: bool,
    // Dialog-specific fields
    pub input_value: String,
}

pub struct MyDialogPlugin;

impl Plugin for MyDialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MyDialogState>()
            .add_systems(EguiPrimaryContextPass, draw_my_dialog);
    }
}

fn draw_my_dialog(
    mut contexts: EguiContexts,
    editor_state: Res<EditorState>,
    mut state: ResMut<MyDialogState>,
) -> Result {
    if !editor_state.ui_enabled || !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let result = draw_centered_dialog(ctx, "Dialog Title", [400.0, 200.0], |ui| {
        ui.label(egui::RichText::new("Description text").color(colors::TEXT_SECONDARY));
        ui.add_space(8.0);

        ui.add(
            egui::TextEdit::singleline(&mut state.input_value)
                .hint_text("Enter value..."),
        );

        ui.add_space(16.0);

        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                return DialogResult::Close;
            }
            if ui.button("OK").clicked() {
                return DialogResult::Confirmed;
            }
            DialogResult::None
        })
        .inner
    });

    match result {
        DialogResult::Confirmed => {
            // Handle confirmation
            state.open = false;
        }
        DialogResult::Close => {
            state.open = false;
        }
        DialogResult::None => {}
    }

    Ok(())
}
```

### Template D: Fuzzy Search Palette (like Command Palette, Find Object)

Use for searchable lists of items.

```rust
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::EditorState;
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, CategorizedItem, PaletteConfig, PaletteResult, PaletteState,
};
use crate::ui::theme::colors;

#[derive(Resource)]
pub struct MySearchState {
    pub open: bool,
    pub palette: PaletteState,
    pub items: Vec<CategorizedItem>,
}

impl Default for MySearchState {
    fn default() -> Self {
        Self {
            open: false,
            palette: PaletteState::default(),
            items: Vec::new(),
        }
    }
}

impl MySearchState {
    pub fn open(&mut self) {
        self.open = true;
        self.palette.reset();
        // Rebuild items list if needed
        self.items = build_items();
    }
}

fn build_items() -> Vec<CategorizedItem> {
    vec![
        CategorizedItem {
            label: "Item One".into(),
            category: "Category A".into(),
            enabled: true,
            suffix: None,
        },
        // ... more items
    ]
}

pub struct MySearchPlugin;

impl Plugin for MySearchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MySearchState>()
            .add_systems(EguiPrimaryContextPass, draw_my_search);
    }
}

fn draw_my_search(
    mut contexts: EguiContexts,
    editor_state: Res<EditorState>,
    mut state: ResMut<MySearchState>,
) -> Result {
    if !editor_state.ui_enabled || !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let config = PaletteConfig {
        title: "SEARCH",
        title_color: colors::ACCENT_BLUE,
        subtitle: "Find items",
        hint_text: "Type to search...",
        action_label: "select",
        size: [400.0, 300.0],
        show_categories: true,
    };

    match draw_fuzzy_palette(ctx, &mut state.palette, &state.items, &config) {
        PaletteResult::Selected(index) => {
            let item = &state.items[index];
            info!("Selected: {}", item.label);
            // Handle selection
            state.open = false;
        }
        PaletteResult::Closed => {
            state.open = false;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}
```

### Template E: Exclusive World Access Panel (like Inspector)

Use when you need to read/write arbitrary components via reflection or need unrestricted world access.

```rust
use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::EditorState;
use crate::selection::Selected;
use crate::ui::theme::{colors, panel, panel_frame};

pub struct MyReflectPanelPlugin;

impl Plugin for MyReflectPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_my_reflect_panel);
    }
}

fn draw_my_reflect_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    // Get selected entities
    let selected_entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Selected>>();
        query.iter(world).collect()
    };

    if selected_entities.is_empty() {
        return;
    }

    // Extract data you need before getting egui context
    let some_data = {
        // ... read from world ...
        "example".to_string()
    };

    // Get egui context
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

    let mut changed = false;
    let mut data_copy = some_data.clone();

    egui::Window::new("Reflect Panel")
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            // Draw UI, modify data_copy, set changed = true
            if ui.text_edit_singleline(&mut data_copy).changed() {
                changed = true;
            }
        });

    // Apply changes back to world
    if changed {
        // world.get_mut::<MyComponent>(entity) ...
    }
}
```

## Theme Reference

### Colors

```rust
use crate::ui::theme::colors;

// Backgrounds
colors::BG_DARKEST       // Text input backgrounds
colors::BG_DARK          // Window backgrounds
colors::BG_MEDIUM        // Striped backgrounds
colors::BG_LIGHT         // Borders
colors::PANEL_BG         // Panel backgrounds

// Text
colors::TEXT_PRIMARY      // Main text (220, 220, 220)
colors::TEXT_SECONDARY    // Secondary text (160, 160, 160)
colors::TEXT_MUTED        // Disabled/hint text (120, 120, 120)

// Accents
colors::ACCENT_BLUE       // Links, selections
colors::ACCENT_GREEN      // Success, add actions
colors::ACCENT_ORANGE     // Warnings, highlights
colors::ACCENT_PURPLE     // Special modes
colors::ACCENT_CYAN       // Measurements

// Selection
colors::SELECTION_BG      // Selected item background
colors::HOVER_BG          // Hover background

// Axis (for transform labels)
colors::AXIS_X / AXIS_Y / AXIS_Z

// Status
colors::STATUS_SUCCESS / STATUS_WARNING / STATUS_ERROR
```

### Frame Helpers

```rust
use crate::ui::theme::{window_frame, panel_frame, popup_frame};

// Floating windows and dialogs
window_frame(&ctx.style())

// Side panels (inspector, hierarchy)
panel_frame(&ctx.style())

// Tooltips and popups
popup_frame(&ctx.style())
```

### Panel Constants

```rust
use crate::ui::theme::panel;

panel::WINDOW_PADDING     // 8.0
panel::STATUS_BAR_HEIGHT   // 24.0
panel::DEFAULT_WIDTH       // 250.0
panel::MIN_WIDTH           // 250.0
panel::MIN_HEIGHT          // 100.0
panel::TITLE_BAR_HEIGHT    // 28.0
panel::BOTTOM_PADDING      // 30.0
```

### Dialog Helpers

```rust
use crate::ui::theme::{draw_centered_dialog, draw_error_dialog, DialogResult};

// Centered modal with custom content
let result = draw_centered_dialog(ctx, "Title", [400.0, 200.0], |ui| {
    // Return DialogResult::Confirmed, Close, or None
});

// Simple error popup
if draw_error_dialog(ctx, "Error", "Message") {
    // Closed
}
```

## Common UI Patterns

### Collapsing Sections

```rust
egui::CollapsingHeader::new(
    egui::RichText::new("Section").strong().color(colors::TEXT_PRIMARY),
)
.default_open(true)
.show(ui, |ui| {
    // Section content
});
```

### Property Rows

```rust
// Label + drag value
ui.horizontal(|ui| {
    ui.label(egui::RichText::new("Speed").color(colors::TEXT_SECONDARY));
    ui.add(egui::DragValue::new(&mut speed).speed(0.1).range(0.0..=100.0));
});

// Label + checkbox
ui.horizontal(|ui| {
    ui.label(egui::RichText::new("Enabled").color(colors::TEXT_SECONDARY));
    ui.checkbox(&mut enabled, "");
});

// Label + color picker
ui.horizontal(|ui| {
    ui.label(egui::RichText::new("Color").color(colors::TEXT_SECONDARY));
    ui.color_edit_button_rgb(&mut color);
});
```

### Grid Layout

```rust
egui::Grid::new("my_grid")
    .num_columns(2)
    .spacing([10.0, 8.0])
    .show(ui, |ui| {
        ui.label("Label:");
        ui.text_edit_singleline(&mut value);
        ui.end_row();
    });
```

### Right-to-Left Layout

```rust
ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
    // Items laid out right-to-left
    ui.label("rightmost");
    ui.label("leftmost");
});
```

### Keyboard Handling

```rust
// Inside draw system, from egui context
let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));

// From Bevy input resource (in system params)
keyboard: Res<ButtonInput<KeyCode>>
if keyboard.just_pressed(KeyCode::KeyF) { ... }
```

### Writing Events

```rust
// Single event writer in params
mut my_events: MessageWriter<MyEvent>,
my_events.write(MyEvent { ... });

// Grouped event writers with SystemParam
#[derive(SystemParam)]
struct PanelEvents<'w> {
    spawn: MessageWriter<'w, SpawnEntityEvent>,
    delete: MessageWriter<'w, DeleteSelectedEvent>,
}
```

## Checklist

- [ ] Panel module created in `src/ui/`
- [ ] Module declared in `src/ui/mod.rs` (`mod my_panel;`)
- [ ] Public items re-exported (`pub use my_panel::*;`)
- [ ] Plugin added to `UiPlugin::build()` in correct section
- [ ] System runs in `EguiPrimaryContextPass` schedule
- [ ] `editor_state.ui_enabled` check at top of draw function
- [ ] Theme colors and frames used consistently
- [ ] State resource initialized with `init_resource` (if applicable)
