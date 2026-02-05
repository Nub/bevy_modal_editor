# Command Palette & Fuzzy Palette

Use this skill when adding a new command to the command palette, or creating a new fuzzy search palette. Examples: "add an align command to the palette", "add a new search palette for materials", "make this action available from the command palette".

There are two patterns covered here:
- **Pattern A**: Add a command to the existing command palette (most common)
- **Pattern B**: Use `draw_fuzzy_palette` for a new searchable list UI

## Key Rule

**Never create standalone palette implementations.** All fuzzy search UIs must use the shared `draw_fuzzy_palette()` from `src/ui/fuzzy_palette.rs`. If a new palette needs a feature that `draw_fuzzy_palette` doesn't support, **add the feature to the shared widget** so all palettes benefit.

## File Locations

- **Command palette**: `src/ui/command_palette.rs`
- **Fuzzy palette widget**: `src/ui/fuzzy_palette.rs` (shared search UI — modify this to add features)
- **Theme colors**: `src/ui/theme.rs`
- **Existing callers**: `find_object.rs`, `material_preset_palette.rs`, `entity_picker.rs`, `asset_browser.rs`, `command_palette.rs`

---

## Pattern A: Add a Command to the Existing Palette

This adds a new action accessible via the `C` key command palette. Touches **1 file** (`src/ui/command_palette.rs`) in **4 locations**.

### Step 1: Add CommandAction Variant

In the `CommandAction` enum (~line 78):

```rust
pub enum CommandAction {
    // ... existing variants ...
    /// Description of what this action does
    MyNewAction,
    // Or with data:
    MyParameterizedAction(f32),
}
```

### Step 2: Add Command Entry

In `CommandRegistry::build_static_commands()` (~line 245), add the command in the appropriate category group:

```rust
self.commands.push(Command {
    name: "My New Action".to_string(),
    keywords: vec!["alias".into(), "synonym".into(), "related".into()],
    category: "Edit",       // Group in the palette list
    action: CommandAction::MyNewAction,
    insertable: false,      // true if this creates an object (shown in Insert mode)
});
```

**Field reference:**
- `name` — Display text, also primary fuzzy match target
- `keywords` — Additional fuzzy match terms (matched at half priority)
- `category` — Group header in the list. Existing categories: `Primitives`, `Blockout`, `Lights`, `Models`, `Splines`, `Effects`, `Scene`, `Hierarchy`, `Camera`, `Camera Marks`, `Help`, `Settings`, `Edit`, `Debug`, `Physics`, `View`, `Snapping`, `Game`
- `insertable` — If `true`, appears in Insert mode palette (`I` key). If `false`, only in Commands mode (`C` key)

### Step 3: Add Insert Mode Handler (If Insertable)

If `insertable: true`, add a match arm in the insert mode block (~line 1052):

```rust
// In Insert mode, send event to create preview entity
if in_insert_mode {
    match &action {
        // ... existing arms ...
        CommandAction::MyNewAction => {
            events.start_insert.write(StartInsertEvent {
                object_type: InsertObjectType::MyEntity,
            });
        }
        _ => {}
    }
}
```

### Step 4: Add Normal Mode Handler

Add a match arm in the normal mode block (~line 1115):

```rust
// Normal mode - execute action immediately
match action {
    // ... existing arms ...
    CommandAction::MyNewAction => {
        // Option A: Write an event
        events.my_event.write(MyEvent);

        // Option B: Modify state directly
        editor_state.some_field = value;

        // Option C: Open a UI panel
        palette_state2.my_panel_state.open = true;

        // Option D: Queue a command (for scene modifications)
        commands.queue(TakeSnapshotCommand {
            description: "My action".to_string(),
        });
        // ... then do the modification
    }
}
```

### Step 5: Add Event Writer (If Needed)

If the action sends an event, add it to the `CommandEvents` SystemParam (~line 32):

```rust
#[derive(SystemParam)]
struct CommandEvents<'w> {
    // ... existing writers ...
    my_event: MessageWriter<'w, MyEvent>,
}
```

Or if it needs a UI state resource, add it to `PaletteState2` (~line 51):

```rust
#[derive(SystemParam)]
struct PaletteState2<'w> {
    // ... existing resources ...
    my_panel_state: ResMut<'w, MyPanelState>,
}
```

### Checklist (Pattern A)

- [ ] `CommandAction` variant added
- [ ] `Command` entry added in `build_static_commands()` with name, keywords, category
- [ ] Insert mode match arm added (if `insertable: true`)
- [ ] Normal mode match arm added
- [ ] Event writer added to `CommandEvents` (if sending an event)
- [ ] State resource added to `PaletteState2` (if toggling UI state)

---

## Pattern B: Use the Shared Fuzzy Palette

All fuzzy search UIs call `draw_fuzzy_palette()` from `src/ui/fuzzy_palette.rs`. This is the **single shared widget** for all searchable lists — command palette, find object, material presets, asset browser, entity picker, etc.

**Do NOT** create custom palette rendering. If you need a feature (e.g. multi-select, custom item rendering, new layout), add it to `draw_fuzzy_palette` and the `PaletteConfig`/`PaletteItem` types in `src/ui/fuzzy_palette.rs` so all callers can benefit.

**Do NOT** add new modes to `PaletteMode` in `command_palette.rs`. New search UIs get their own module that calls `draw_fuzzy_palette` directly.

### Step 1: Implement `PaletteItem` for Your Items

```rust
struct MyItem {
    name: String,
    category: String,
}

impl PaletteItem for MyItem {
    fn label(&self) -> &str { &self.name }
    fn category(&self) -> Option<&str> { Some(&self.category) }
    // All other methods have sensible defaults
}
```

### Step 2: Create State and Draw System

```rust
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, PaletteConfig, PaletteResult, PaletteState,
};

#[derive(Resource)]
pub struct MySearchState {
    pub open: bool,
    pub palette: PaletteState,
    pub items: Vec<MyItem>,
}

impl MySearchState {
    pub fn open(&mut self) {
        self.open = true;
        self.palette.reset();
        self.items = build_items();
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
        title: "MY SEARCH",
        title_color: colors::ACCENT_BLUE,
        subtitle: "Find items",
        hint_text: "Type to search...",
        action_label: "select",
        size: [400.0, 300.0],
        show_categories: true,
        ..Default::default()
    };

    match draw_fuzzy_palette(ctx, &mut state.palette, &state.items, config) {
        PaletteResult::Selected(index) => {
            let item = &state.items[index];
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

### PaletteItem Trait Reference

```rust
pub trait PaletteItem {
    /// Primary text for display and fuzzy matching (REQUIRED)
    fn label(&self) -> &str;

    /// Additional fuzzy match terms (matched at half priority)
    fn keywords(&self) -> &[String] { &[] }

    /// Category for grouping (shown as header when show_categories is true)
    fn category(&self) -> Option<&str> { None }

    /// Whether this item can be selected (grayed out if false)
    fn is_enabled(&self) -> bool { true }

    /// Text shown after the label (e.g., "(no default)")
    fn suffix(&self) -> Option<&str> { None }

    /// If true, item always appears at top regardless of query
    fn always_visible(&self) -> bool { false }
}
```

**Pre-built item types** (in `fuzzy_palette.rs`):
- `SimpleItem` — Just a label
- `CategorizedItem` — Label + category + enabled + suffix
- `KeywordItem` — Label + keywords + optional category

### PaletteConfig Reference

```rust
PaletteConfig {
    title: "TITLE",              // Mode indicator text (uppercase convention)
    title_color: colors::ACCENT_BLUE,  // Semantic accent color
    subtitle: "description",     // Shown after title, muted
    hint_text: "Type to...",     // Search input placeholder
    action_label: "select",      // Shown in "Enter to {action_label}" footer
    size: [400.0, 300.0],        // Window size [width, height]
    show_categories: true,       // Show category group headers
    preview_panel: None,         // Optional right-side preview closure
    preview_width: 230.0,        // Width of preview panel if used
}
```

**Common sizes:**
- Standard palette: `[400.0, 300.0]`
- With preview panel: `[400.0, 350.0]` (preview adds width automatically)

**Title color conventions:**
- Blue (`ACCENT_BLUE`) — General search/commands
- Green (`ACCENT_GREEN`) — Adding/creating (Insert mode, Add Component)
- Purple (`ACCENT_PURPLE`) — Inspection/editing
- Red (`STATUS_ERROR`) — Destructive (Remove Component)

### Preview Panel

The fuzzy palette supports an optional right-side preview panel. When `preview_panel` is `Some(...)`, `draw_fuzzy_palette` automatically creates a two-column layout: scrollable item list on the left, preview on the right. The palette window width expands by `preview_width + 8.0` to accommodate it.

**How it works:**
- `preview_panel` is a `Box<dyn FnOnce(&mut egui::Ui)>` closure — it draws into the right column
- The closure is consumed each frame, so it must be rebuilt every frame
- You track which item is highlighted, and build the closure with that item's data
- The `size` in `PaletteConfig` is the *left column* size — total width = `size[0] + preview_width + 8.0`

**Existing usage:**
- `src/ui/material_preset_palette.rs` — Shows a material sphere preview image
- `src/ui/asset_browser.rs` — Shows texture thumbnail when picking textures

#### Pattern: Static Data Preview

For previewing text/properties of the highlighted item:

```rust
use crate::ui::fuzzy_palette::fuzzy_filter;

// Determine which item is currently highlighted
let filtered = fuzzy_filter(&items, &palette_state.query);
let highlighted = filtered
    .get(palette_state.selected_index)
    .map(|fi| &items[fi.index]);

// Build the preview closure (captures data by value/clone)
let preview_data = highlighted.map(|item| (item.name.clone(), item.description.clone()));

let preview_panel: Box<dyn FnOnce(&mut egui::Ui) + '_> = Box::new(move |ui| {
    ui.label(
        egui::RichText::new("Preview")
            .small()
            .strong()
            .color(colors::TEXT_SECONDARY),
    );
    ui.add_space(4.0);

    if let Some((name, description)) = &preview_data {
        ui.label(egui::RichText::new(name).color(colors::TEXT_PRIMARY).strong());
        ui.add_space(4.0);
        ui.label(egui::RichText::new(description).color(colors::TEXT_SECONDARY));
    } else {
        ui.label(
            egui::RichText::new("Nothing selected")
                .color(colors::TEXT_MUTED)
                .italics(),
        );
    }
});

let config = PaletteConfig {
    // ...
    size: [342.0, 340.0],         // Left column size
    preview_panel: Some(preview_panel),
    preview_width: 230.0,         // Default, usually fine
    ..Default::default()
};
```

#### Pattern: Image/Texture Preview

For showing a rendered preview that updates as selection changes. Requires tracking the previously previewed item to avoid re-loading every frame.

State fields needed:

```rust
pub struct MyPaletteState {
    pub palette_state: PaletteState,
    pub prev_previewed: Option<String>,      // Track what's being previewed
    pub preview_texture_id: Option<egui::TextureId>,  // Resolved egui texture
    pub preview_handle: Option<Handle<Image>>,         // Keep Bevy handle alive
}
```

Build the preview, updating only when the highlighted item changes:

```rust
// Resolve currently highlighted item
let filtered = fuzzy_filter(&items, &state.palette_state.query);
let current_name = filtered
    .get(state.palette_state.selected_index)
    .map(|fi| fi.item.name.clone());

// Update preview only when highlighted item changes
if current_name != state.prev_previewed {
    // Clean up old texture
    if let Some(ref old_handle) = state.preview_handle.take() {
        contexts.remove_image(old_handle);
        state.preview_texture_id = None;
    }

    // Load new preview
    if let Some(ref name) = current_name {
        let handle: Handle<Image> = asset_server.load(format!("previews/{}.png", name));
        let tex_id = contexts.add_image(EguiTextureHandle::Strong(handle.clone()));
        state.preview_handle = Some(handle);
        state.preview_texture_id = Some(tex_id);
    }

    state.prev_previewed = current_name.clone();
}

// Build closure with current texture
let preview_texture_id = state.preview_texture_id;
let preview_name = state.prev_previewed.clone();

let preview_panel: Box<dyn FnOnce(&mut egui::Ui) + '_> = Box::new(move |ui| {
    ui.label(
        egui::RichText::new("Preview")
            .small()
            .strong()
            .color(colors::TEXT_SECONDARY),
    );
    ui.add_space(4.0);

    if let Some(tex_id) = preview_texture_id {
        let size = ui.available_width().min(220.0);
        ui.image(egui::load::SizedTexture::new(tex_id, [size, size]));
    } else {
        ui.label(
            egui::RichText::new("Preview loading...")
                .color(colors::TEXT_MUTED)
                .italics(),
        );
    }

    if let Some(ref name) = preview_name {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(name).color(colors::TEXT_PRIMARY).strong());
    }
});
```

#### Preview Panel Style Conventions

- Always start with a `"Preview"` header: `.small().strong().color(TEXT_SECONDARY)`
- `ui.add_space(4.0)` between header and content
- Image size: `ui.available_width().min(220.0)` for square thumbnails
- Item name below image: `.color(TEXT_PRIMARY).strong()`
- Loading state: `"Preview loading..."` in `.color(TEXT_MUTED).italics()`
- No selection: `"Nothing selected"` in `.color(TEXT_MUTED).italics()`

### Extending the Shared Widget

If `draw_fuzzy_palette` doesn't support a feature you need (e.g. multi-select, custom row rendering, action buttons), **add it to `src/ui/fuzzy_palette.rs`** — not to your caller.

Typical extension points:
- **New `PaletteConfig` field** — add an `Option<...>` field with a default of `None` so existing callers aren't affected
- **New `PaletteItem` method** — add with a default implementation so existing item types don't break
- **New `PaletteResult` variant** — for new interaction types (e.g. secondary action)
- **Layout changes** — modify `draw_fuzzy_palette` internals, test against all existing callers

After extending, update this skill file to document the new capability.

### Checklist (Pattern B)

- [ ] `PaletteItem` implemented for items (or use pre-built `SimpleItem`/`CategorizedItem`/`KeywordItem`)
- [ ] State resource with `open: bool` + `PaletteState` + items
- [ ] `open()` method resets palette state and rebuilds items
- [ ] Draw system checks `ui_enabled` and `state.open`
- [ ] Calls `draw_fuzzy_palette` — no custom palette rendering
- [ ] System registered in `EguiPrimaryContextPass`
- [ ] Plugin registered (see `ui-panel` and `add-plugin` skills)
- [ ] Preview state fields added (if using preview panel)
- [ ] Old texture cleaned up on highlight change (if using image preview)
- [ ] If a new feature was needed, it was added to `src/ui/fuzzy_palette.rs` (not the caller)
