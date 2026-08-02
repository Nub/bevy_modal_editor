//! The editor inside the game (spec §1) — only compiled with `--features editor`.
//!
//! M1 scope: the kernel (modes, resolver, keymaps), the command palette, a status bar,
//! and an nvim-style which-key popup. Chrome follows the standing design bar (spec §7):
//! themed surfaces, shared style scale, never floating text over the render.

use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::{dark_theme::create_dark_theme, theme::UiTheme, tokens, FeathersPlugins};
use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;

use crate::game::{GameInputActive, Primitive, PrimitiveKind};
use crate::ui_style::{self as style, UiFonts};

#[derive(Component, Default, Clone)]
struct StatusBar;
#[derive(Component, Default, Clone)]
struct StatusModeText;
#[derive(Component, Default, Clone)]
struct StatusHint;
#[derive(Component, Default, Clone)]
struct StatusKeys;
#[derive(Component, Default, Clone)]
struct StatusDirty;

/// Transient status feedback (save/load results). Shown in the keys slot when no
/// sequence is pending.
#[derive(Resource, Default)]
struct StatusFlash {
    text: String,
    success: bool,
    until: f32,
}
#[derive(Component, Default, Clone)]
struct WhichKeyPanel;

/// The game's editor-facing registration: which components serialize, what can be
/// placed. Lives editor-side; the game module stays editor-free.
struct GameFeature;

fn cube_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        Box::new(Transform::from_translation(position + Vec3::Y * 0.5))
            .into_partial_reflect(),
        Box::new(Primitive { kind: PrimitiveKind::Cube, size: 1.0 }).into_partial_reflect(),
    ]
}

fn sphere_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    use bevy::reflect::PartialReflect;
    vec![
        Box::new(Transform::from_translation(position + Vec3::Y * 0.5))
            .into_partial_reflect(),
        Box::new(Primitive { kind: PrimitiveKind::Sphere, size: 1.0 }).into_partial_reflect(),
    ]
}

impl EditorFeature for GameFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("template-game", "Template Game")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.component::<Transform>()
            .component::<Primitive>()
            .entity_kind(EntityKindDef {
                id: EntityKindId::new_static("primitive.cube"),
                display_name: "Cube",
                components: cube_components,
            })
            .entity_kind(EntityKindDef {
                id: EntityKindId::new_static("primitive.sphere"),
                display_name: "Sphere",
                components: sphere_components,
            });
    }
}

/// Ghost styling for insert previews: translucent accent (applied after the game's
/// regenerate observer attaches the normal material).
#[derive(Component)]
struct GhostApplied;

#[derive(Resource)]
struct GhostMaterial(Handle<StandardMaterial>);

fn init_ghost_material(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(GhostMaterial(materials.add(StandardMaterial {
        base_color: Color::srgba(0.35, 0.62, 1.0, 0.45),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    })));
}

#[allow(clippy::type_complexity)]
fn apply_ghost_material(
    ghost: Res<GhostMaterial>,
    mut previews: Query<
        (Entity, &mut MeshMaterial3d<StandardMaterial>),
        (With<InsertPreview>, Without<GhostApplied>),
    >,
    mut commands: Commands,
) {
    for (entity, mut material) in &mut previews {
        material.0 = ghost.0.clone();
        commands.entity(entity).insert(GhostApplied);
    }
}

pub struct EditorOverlayPlugin;

impl Plugin for EditorOverlayPlugin {
    fn build(&self, app: &mut App) {
        // User keymap layer (M1 acceptance: rebind without recompiling): overrides
        // in ./editor-keymap.ron win over registry defaults; delete to restore.
        app.insert_resource(KeymapPaths { user: Some("editor-keymap.ron".into()) });
        app.add_plugins(FeathersPlugins)
            .add_plugins(bevy::picking::mesh_picking::MeshPickingPlugin)
            .insert_resource(UiTheme(create_dark_theme()));
        app.add_editor_feature(GameFeature);
        app.add_plugins((EditorCorePlugin, editor_scene::EditorScenePlugin, crate::palette::PalettePlugin))
            .add_systems(
                Update,
                (
                    sync_game_input,
                    apply_ghost_material,
                    collect_io_feedback,
                    compute_which_key,
                    update_statusbar,
                    rebuild_which_key,
                )
                    .chain()
                    .in_set(editor_core::EditorSet::Sync),
            );
        app.init_resource::<WhichKey>();
        app.init_resource::<StatusFlash>();
        app.add_systems(
            Startup,
            (style::load_ui_fonts, init_ghost_material, spawn_statusbar, spawn_which_key).chain(),
        );
    }
}

fn spawn_statusbar(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            StatusBar,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(0),
                height: px(style::BAR_HEIGHT),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(style::space::M),
                padding: UiRect::horizontal(px(style::space::M)),
                ..default()
            },
            ThemeBackgroundColor(tokens::PANE_HEADER_BG),
            GlobalZIndex(100),
            Visibility::Hidden,
        ))
        .with_children(|bar| {
            bar.spawn((
                Node {
                    padding: UiRect::axes(px(style::space::S), px(2.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(px(style::radius::S)),
                    ..default()
                },
                BackgroundColor(style::color::accent()),
            ))
            .with_children(|chip| {
                chip.spawn((
                    StatusModeText,
                    Text::new("NORMAL"),
                    style::sans_medium(&fonts, style::font_size::XS),
                    TextColor(style::color::TEXT_ON_ACCENT),
                ));
            });
            bar.spawn((
                StatusDirty,
                Text::new("●"),
                style::sans(&fonts, style::font_size::S),
                TextColor(style::color::TEXT_WARN),
                Visibility::Hidden,
            ));
            bar.spawn((
                StatusHint,
                Text::new(""),
                ThemedText,
                style::sans(&fonts, style::font_size::S),
                TextColor(style::color::TEXT_DIM),
            ));
            bar.spawn(Node { flex_grow: 1.0, ..default() });
            bar.spawn((
                StatusKeys,
                Text::new(""),
                style::mono(&fonts, style::font_size::S),
                TextColor(style::color::TEXT_KEYS),
            ));
        });
}

// ---------------------------------------------------------------------------
// Which-key: an nvim-style popup above the status bar. Shown while a prefix is
// pending (its continuations) AND after an unbound press (everything available in
// the current context — teach, don't scold).
// ---------------------------------------------------------------------------

#[derive(Default, PartialEq, Clone)]
struct WhichKeyContent {
    header: String,
    header_warn: bool,
    entries: Vec<(String, String)>, // (key glyphs, action name)
}

#[derive(Resource, Default)]
struct WhichKey {
    content: Option<WhichKeyContent>,
    /// For the unbound case: auto-dismiss deadline (seconds of app time).
    until: Option<f32>,
    dirty: bool,
}

fn spawn_which_key(mut commands: Commands) {
    commands.spawn((
        WhichKeyPanel,
        Node {
            position_type: PositionType::Absolute,
            left: px(style::space::M),
            right: px(style::space::M),
            bottom: px(style::BAR_HEIGHT + style::space::S),
            flex_direction: FlexDirection::Column,
            row_gap: px(style::space::S),
            padding: UiRect::all(px(style::space::M)),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(style::radius::L)),
            ..default()
        },
        ThemeBackgroundColor(tokens::WINDOW_BG),
        BorderColor::all(style::HAIRLINE),
        style::floating_shadow(),
        GlobalZIndex(150),
        Visibility::Hidden,
    ));
}

fn entries_for(
    keymap: &ResolvedKeymap,
    catalog: &ActionCatalog,
    contexts: &[ContextId],
    pending: &[editor_api::keymap::Chord],
) -> Vec<(String, String)> {
    which_key_continuations(keymap, contexts, pending)
        .iter()
        .map(|(chord, binding, action)| {
            let name = catalog.get(action).map(|d| d.name.as_ref()).unwrap_or(action.as_str());
            // Show the immediate next key; if the binding goes deeper, mark it as a
            // group with an ellipsis (nvim-which-key idiom).
            let deeper = binding.0.len() > pending.len() + 1;
            let key = if deeper {
                format!("{} …", style::pretty_chord(chord))
            } else {
                style::pretty_chord(chord)
            };
            (key, name.to_string())
        })
        .collect()
}

fn compute_which_key(
    state: Res<EditorState>,
    mode: Res<CurrentMode>,
    overlay: Res<OverlayContext>,
    pending: Res<PendingKeys>,
    keymap: Res<ResolvedKeymap>,
    catalog: Res<ActionCatalog>,
    mut unresolved: MessageReader<KeysUnresolved>,
    time: Res<Time>,
    mut which_key: ResMut<WhichKey>,
) {
    let now = time.elapsed_secs();
    let contexts = active_contexts(&state, &mode, &overlay);

    let next = if !state.active {
        None
    } else if !pending.0.is_empty() {
        // A live prefix: show its continuations.
        Some((
            WhichKeyContent {
                header: style::pretty_chords(&pending.0),
                header_warn: false,
                entries: entries_for(&keymap, &catalog, &contexts, &pending.0),
            },
            None,
        ))
    } else if let Some(last) = unresolved.read().last() {
        // Unbound press: teach what IS available in this context.
        Some((
            WhichKeyContent {
                header: format!(
                    "{}  ·  unbound — available here:",
                    style::pretty_chords(&last.0)
                ),
                header_warn: true,
                entries: entries_for(&keymap, &catalog, &contexts, &[]),
            },
            Some(now + 3.0),
        ))
    } else if which_key.until.is_some_and(|until| now < until) {
        return; // keep showing the unbound popup until its deadline
    } else {
        None
    };

    match next {
        Some((content, until)) => {
            if which_key.content.as_ref() != Some(&content) {
                which_key.content = Some(content);
                which_key.dirty = true;
            }
            which_key.until = until;
        }
        None => {
            if which_key.content.is_some() {
                which_key.content = None;
                which_key.until = None;
                which_key.dirty = true;
            }
        }
    }
}

fn rebuild_which_key(
    mut which_key: ResMut<WhichKey>,
    panel: Single<(Entity, &mut Visibility), With<WhichKeyPanel>>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
) {
    if !which_key.dirty {
        return;
    }
    which_key.dirty = false;
    let (panel_entity, mut visibility) = panel.into_inner();
    commands.entity(panel_entity).despawn_related::<Children>();

    let Some(content) = &which_key.content else {
        *visibility = Visibility::Hidden;
        return;
    };
    *visibility = Visibility::Visible;

    let header = content.header.clone();
    let header_color =
        if content.header_warn { style::color::TEXT_WARN } else { style::color::TEXT_DIM };
    let entries = content.entries.clone();
    let key_font = style::mono(&fonts, style::font_size::S);

    commands.entity(panel_entity).with_children(|panel| {
        panel.spawn((
            Text::new(header),
            style::mono(&fonts, style::font_size::XS),
            TextColor(header_color),
        ));
        panel
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(style::space::M),
                row_gap: px(style::space::XS),
                ..default()
            })
            .with_children(|grid| {
                for (key, name) in entries {
                    grid.spawn(Node {
                        width: px(220.0),
                        align_items: AlignItems::Center,
                        column_gap: px(style::space::S),
                        ..default()
                    })
                    .with_children(|cell| {
                        cell.spawn((
                            Node {
                                min_width: px(28.0),
                                justify_content: JustifyContent::Center,
                                padding: UiRect::axes(px(style::space::XS), px(1.0)),
                                border_radius: BorderRadius::all(px(style::radius::S)),
                                ..default()
                            },
                            BackgroundColor(style::color::selection()),
                        ))
                        .with_children(|badge| {
                            badge.spawn((
                                Text::new(key),
                                key_font.clone(),
                                TextColor(style::color::TEXT_KEYS),
                            ));
                        });
                        cell.spawn((
                            Text::new(name),
                            style::sans(&fonts, style::font_size::S),
                            TextColor(style::color::TEXT_DIM),
                        ));
                    });
                }
                if content.entries.is_empty() {
                    grid.spawn((
                        Text::new("nothing bound in this context"),
                        style::sans(&fonts, style::font_size::S),
                        TextColor(style::color::TEXT_DIM),
                    ));
                }
            });
    });
}

fn collect_io_feedback(
    mut reader: MessageReader<editor_scene::SceneIoFeedback>,
    time: Res<Time>,
    mut flash: ResMut<StatusFlash>,
) {
    for feedback in reader.read() {
        flash.text = feedback.message.clone();
        flash.success = feedback.success;
        flash.until = time.elapsed_secs() + 3.0;
    }
}

/// The editor owns input while active; the game module knows nothing about the
/// editor — it just honors `GameInputActive`.
fn sync_game_input(state: Res<EditorState>, mut game_input: ResMut<GameInputActive>) {
    let game_owns = !state.active;
    if game_input.0 != game_owns {
        game_input.0 = game_owns;
    }
}

#[allow(clippy::type_complexity)]
fn update_statusbar(
    state: Res<EditorState>,
    mode: Res<CurrentMode>,
    modes: Res<Modes>,
    pending: Res<PendingKeys>,
    gesture: Res<MoveGesture>,
    flash: Res<StatusFlash>,
    dirty: Res<editor_scene::SceneDirty>,
    time: Res<Time>,
    mut bar: Query<&mut Visibility, With<StatusBar>>,
    mut mode_text: Query<
        &mut Text,
        (With<StatusModeText>, Without<StatusHint>, Without<StatusKeys>),
    >,
    mut hint_text: Query<
        &mut Text,
        (With<StatusHint>, Without<StatusModeText>, Without<StatusKeys>),
    >,
    mut keys_text: Query<
        (&mut Text, &mut TextColor),
        (With<StatusKeys>, Without<StatusModeText>, Without<StatusHint>),
    >,
    mut dirty_dot: Query<
        &mut Visibility,
        (With<StatusDirty>, Without<StatusBar>),
    >,
) {
    for mut visibility in &mut bar {
        *visibility = if state.active { Visibility::Visible } else { Visibility::Hidden };
    }
    if !state.active {
        return;
    }

    // An active gesture owns the chip + hint (owner feedback: "w" must show state).
    let gesture_active = !matches!(*gesture, MoveGesture::Idle);
    let mode_def = modes.get(&mode.0);
    for mut text in &mut mode_text {
        let name = if gesture_active {
            "MOVE".to_string()
        } else {
            mode_def.map(|m| m.name.to_uppercase()).unwrap_or_else(|| "?".into())
        };
        if text.0 != name {
            text.0 = name;
        }
    }
    for mut text in &mut hint_text {
        let hint = if gesture_active {
            "x/y/z constrain · click ⏎ commit · ⎋ cancel".to_string()
        } else {
            mode_def.map(|m| m.statusline_hint.to_string()).unwrap_or_default()
        };
        if text.0 != hint {
            text.0 = hint;
        }
    }
    for mut visibility in &mut dirty_dot {
        *visibility = if dirty.0 { Visibility::Visible } else { Visibility::Hidden };
    }
    // Keys slot: pending glyphs win; otherwise transient save/load feedback.
    for (mut text, mut color) in &mut keys_text {
        if !pending.0.is_empty() {
            let pending_text = style::pretty_chords(&pending.0);
            if text.0 != pending_text {
                text.0 = pending_text;
                color.0 = style::color::TEXT_KEYS;
            }
        } else if time.elapsed_secs() < flash.until {
            if text.0 != flash.text {
                text.0 = flash.text.clone();
                color.0 = if flash.success {
                    Color::srgb(0.55, 0.78, 0.55)
                } else {
                    style::color::TEXT_WARN
                };
            }
        } else if !text.0.is_empty() {
            text.0.clear();
        }
    }
}
