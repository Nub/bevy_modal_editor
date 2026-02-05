# UI Design Conventions

Use this skill when building or modifying any egui-based UI in the editor. Ensures visual consistency, correct interaction patterns, and adherence to the established design language.

This is a **design reference**, not a code scaffold. For boilerplate and file registration, use `ui-panel`.

## Design Principles

1. **Dark, minimal, information-dense** — No wasted space. Dark backgrounds, subtle borders, compact rows.
2. **Color means something** — Every accent color has a semantic role. Don't use colors decoratively.
3. **Keyboard-first** — All dialogs support Enter/Escape. Lists support arrow keys. Focus is managed explicitly.
4. **Consistent grid layout** — Property editing always uses 2-column grids with muted labels on the left.

---

## Color Semantics

Use colors from `crate::ui::theme::colors`. Never hardcode `Color32` values in UI code.

### When to Use Each Accent

| Color | Constant | Meaning | Used For |
|-------|----------|---------|----------|
| Blue | `ACCENT_BLUE` | Primary / interactive | Links, selections, View mode, keyboard hints, hover borders |
| Green | `ACCENT_GREEN` | Additive / success | Add buttons, Insert mode, success states |
| Orange | `ACCENT_ORANGE` | Edit / warning | Edit mode, warnings, highlighted values |
| Purple | `ACCENT_PURPLE` | Inspector / special | Inspector header, Material mode |
| Cyan | `ACCENT_CYAN` | Reference / info | Entity references, measurements, Hierarchy mode |

### Text Color Rules

| Color | When |
|-------|------|
| `TEXT_PRIMARY` | Main labels, selected items, section headers, user-entered values |
| `TEXT_SECONDARY` | Non-selected list items, secondary descriptions |
| `TEXT_MUTED` | Grid column labels, hints, disabled text, placeholder text |

### Status Colors

| Color | When |
|-------|------|
| `STATUS_SUCCESS` | Physics ON, Playing state |
| `STATUS_WARNING` | Modified indicator, Paused state |
| `STATUS_ERROR` | Physics OFF, error states |

### Background Rules

- `BG_DARK` / `PANEL_BG` — Window and panel fills (use via `panel_frame()` / `window_frame()`)
- `BG_DARKEST` — Text input backgrounds only (set globally, don't override)
- `BG_MEDIUM` — Striped table rows, section fills
- `SELECTION_BG` — Selected items in lists
- `HOVER_BG` — Hover states on interactive elements
- Never construct frames manually — use `panel_frame(&ctx.style())`, `window_frame(&ctx.style())`, or `popup_frame(&ctx.style())`

---

## Layout Rules

### The Sacred Grid

All property editing uses this exact configuration:

```rust
egui::Grid::new(unique_id)
    .num_columns(2)
    .spacing([8.0, 4.0])  // NEVER change these values
```

- **Column 1**: Muted label via `grid_label(ui, "Label")`
- **Column 2**: Widget(s) — DragValue, checkbox, color picker, ComboBox, or horizontal group
- Every row ends with `ui.end_row()`

**Exception**: Settings panel uses `[10.0, 8.0]` spacing for its wider layout.

### Property Row Patterns

**Scalar value** — DragValue at standard width:
```rust
grid_label(ui, "Metallic");
value_slider(ui, &mut val, 0.0..=1.0);
ui.end_row();
```

**Boolean** — Checkbox with no label (grid label serves as label):
```rust
grid_label(ui, "Shadows");
ui.checkbox(&mut enabled, "");
ui.end_row();
```

**Color** — Inline color button:
```rust
grid_label(ui, "Color");
ui.color_edit_button_rgba_unmultiplied(&mut color_arr);
ui.end_row();
```

**Vec3 / Position** — XYZ row with axis-colored labels:
```rust
grid_label(ui, "Position");
xyz_row(ui, &mut [x, y, z], 0.1);  // Labels colored AXIS_X/Y/Z
ui.end_row();
```

**Enum / Selection** — ComboBox:
```rust
grid_label(ui, "Type");
egui::ComboBox::from_id_salt("type_combo")
    .selected_text(current.label())
    .show_ui(ui, |ui| { /* selectable_value per option */ });
ui.end_row();
```

**Compound** — Wrap in `ui.horizontal()`:
```rust
grid_label(ui, "UV Scale");
ui.horizontal(|ui| {
    ui.label(RichText::new("U").color(TEXT_MUTED));
    ui.add_sized([DRAG_VALUE_WIDTH, h], DragValue::new(&mut u));
    ui.label(RichText::new("V").color(TEXT_MUTED));
    ui.add_sized([DRAG_VALUE_WIDTH, h], DragValue::new(&mut v));
});
ui.end_row();
```

### Spacing Values

These are fixed — don't invent new spacing values:

| Value | Where |
|-------|-------|
| `2.0` | After collapsing headers |
| `4.0` | After separators, between elements |
| `8.0` | Between major sections, before separators |
| `10.0` | Help text gaps in palettes |
| `20.0` | Empty state top spacing |

**Separator pattern** — always add space after:
```rust
ui.separator();
ui.add_space(4.0);
```

**Section break** — extra space before:
```rust
ui.add_space(8.0);
ui.separator();
ui.add_space(4.0);
```

### Sections

Use `section_header()` or manual `CollapsingHeader` with this styling:

```rust
egui::CollapsingHeader::new(
    egui::RichText::new("Section Name")
        .strong()
        .color(colors::TEXT_PRIMARY)
)
.default_open(true)  // or false for secondary sections
```

Always put a grid inside sections, not loose widgets.

---

## Widget Selection Guide

### When to Use What

| Widget | Use When |
|--------|----------|
| `DragValue` | Precise numeric input (always sized to `DRAG_VALUE_WIDTH`) |
| `Slider` | Visual adjustment of a bounded range |
| `value_slider()` | Both — DragValue + Slider combined (preferred for 0..1 ranges) |
| `TextEdit::singleline` | Text input (names, search, file paths) |
| `checkbox` | Boolean toggle |
| `ComboBox` | Enum/option selection (3+ choices) |
| `selectable_value` | Inside ComboBox dropdowns |
| `selectable_label` | List items (hierarchy, search results) |
| `Button` | Discrete actions |
| `small_button` | Inline actions (remove, browse, clear) |
| `color_edit_button_*` | Color picker |

### DragValue Speeds

| Domain | Speed | Decimals |
|--------|-------|----------|
| PBR properties (0-1) | `0.01` | 2 |
| Positions, UV scale | `0.1` | 2 |
| Rotation (degrees) | `1.0` | 0 |
| Intensity, range | `1.0` | 0 |

### DragValue Sizing

Always use the constant, never hardcode:
```rust
ui.add_sized(
    [DRAG_VALUE_WIDTH, ui.spacing().interact_size.y],
    egui::DragValue::new(&mut value).speed(0.01)
)
```

---

## Text Styling Patterns

### Headers and Labels

```rust
// Section header (inside CollapsingHeader)
RichText::new("Section").strong().color(TEXT_PRIMARY)

// Mode indicator (status bar)
RichText::new("[EDIT]").strong().color(ACCENT_ORANGE)

// Grid label
grid_label(ui, "Property")  // -> RichText::new(...).color(TEXT_MUTED)

// Keyboard hint
RichText::new("Enter").small().strong().color(ACCENT_BLUE)
```

### Empty States

Always centered, italic, muted:
```rust
ui.add_space(20.0);
ui.vertical_centered(|ui| {
    ui.label(RichText::new("No entity selected").color(TEXT_MUTED).italics());
    ui.add_space(8.0);
    ui.label(RichText::new("Helpful instruction text").small().color(TEXT_MUTED));
});
```

### Button Text

```rust
// Add/create action — green
RichText::new("+ Add Component").color(ACCENT_GREEN)

// Normal action — default styling
ui.button("OK")
ui.button("Cancel")

// Inline remove — small, no color
ui.small_button("X").on_hover_text("Remove")
```

---

## Interaction Patterns

### Dialog Lifecycle

1. **Open**: Set `state.open = true`, `state.just_opened = true`, clear query/index
2. **First frame**: `response.request_focus()`, set `just_opened = false`
3. **Close**: Escape key → `DialogResult::Close`, or action → `DialogResult::Confirmed`

### Keyboard Navigation in Lists

- **Arrow Down/Up**: Move `selected_index`, clamped to bounds
- **Enter**: Confirm selection (only if item `is_enabled()`)
- **Escape**: Close/cancel
- **Typing**: Filter items (in search palettes)
- **Scroll**: `response.scroll_to_me(Some(egui::Align::Center))` for selected item

### Focus Management

- Dialogs request focus on their text input when opened
- Use `egui::Id::new("field_name")` for specific fields that need programmatic focus
- Check `ctx.wants_keyboard_input()` before processing editor hotkeys in UI

### Hover Effects

- **Buttons**: Use `.on_hover_text("description")` for non-obvious actions
- **Drop targets**: Paint a `ACCENT_BLUE` border stroke on hover during drag
- **List items**: `SELECTION_BG` for selected, `HOVER_BG` on hover (egui handles this automatically with `selectable_label`)

---

## Panel Structure

### Docked Panels

- **Left**: Hierarchy — `Align2::LEFT_TOP`, offset `[WINDOW_PADDING, WINDOW_PADDING]`
- **Right**: Inspector — `Align2::RIGHT_TOP`, offset `[-WINDOW_PADDING, WINDOW_PADDING]`
- Both use `panel_frame()`, `DEFAULT_WIDTH` (250), and calculate available height:

```rust
let available_height = ctx.content_rect().height()
    - panel::STATUS_BAR_HEIGHT
    - panel::WINDOW_PADDING * 2.0;
```

### Floating Windows

- Settings, material editor — `window_frame()`, centered or offset
- No title bar for inline info windows (edit mode info)
- Always `collapsible(false)` for non-docked windows

### Modal Dialogs

- Centered: `Align2::CENTER_CENTER`
- Use `draw_centered_dialog()` for standard confirm/cancel
- Use `draw_error_dialog()` for simple errors
- `Order::Foreground` for palettes/search

### Status Bar

- `TopBottomPanel::bottom`, height `STATUS_BAR_HEIGHT` (24)
- Inner margin: `Margin::symmetric(12, 6)`
- Left side: mode, operation, context info
- Right side: `Layout::right_to_left` for file name, counts, measurements

---

## Icons

Uses Nerd Font codepoints. Key icons:

| Icon | Codepoint | Meaning |
|------|-----------|---------|
| Folder | `\u{f07b}` | Groups |
| File | `\u{f15b}` | Generic entities |
| Lightbulb | `\u{f0eb}` | Point lights |
| Sun | `\u{f185}` | Directional lights |
| Cube | `\u{f1b2}` | Cube primitive |
| Circle | `\u{f111}` | Sphere primitive |
| Lock | `\u{f023}` | Locked entities |
| Ruler | `\u{f546}` | Distance measurement |
| Dot | `\u{f111}` | Modified indicator |

Define icon constants at the top of the file, not inline.

---

## Common Mistakes to Avoid

1. **Don't hardcode colors** — Always use `colors::*` constants
2. **Don't change grid spacing** — `[8.0, 4.0]` is universal (except Settings)
3. **Don't skip `ui_enabled` check** — Every draw system must check this first
4. **Don't construct frames manually** — Use `panel_frame()`, `window_frame()`, `popup_frame()`
5. **Don't forget separator spacing** — Always `ui.add_space(4.0)` after `ui.separator()`
6. **Don't hardcode DragValue width** — Use `DRAG_VALUE_WIDTH` constant
7. **Don't use `#[derive(Event)]`** — Use `#[derive(Message)]` with `MessageReader`/`MessageWriter`
8. **Don't forget `just_opened` focus** — Dialogs must focus their input on first frame
9. **Don't add emojis** — Use Nerd Font icons or plain text
10. **Don't invent new spacing values** — Use only 2.0, 4.0, 8.0, 10.0, 20.0
