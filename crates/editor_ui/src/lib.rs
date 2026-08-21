//! Editor chrome (spec §2 `editor_ui`): the canonical editor UI every game gets by
//! depending on this crate — command palette, status bar, which-key popup, and the
//! style system. Games contribute *content* (kinds, components, features) through
//! `editor_api`; they never own chrome.
//!
//! `EditorUiPlugin` is the single editor entry point for a game binary: it brings
//! the kernel (`editor_core`), scene I/O (`editor_scene`), feathers theming, and
//! all chrome surfaces. Fonts ship embedded — games don't copy asset files.

pub mod style;

mod appear;
mod assets;
mod confirm;
mod dock;
mod feature_gizmos;
mod ghost;
mod grid;
mod hierarchy;
mod inspector;
mod light_gizmo;
mod list;
mod marquee;
mod material_editor;
mod open_indicator;
mod outline;
mod palette;
mod palette_engine;
mod palette_preview;
mod preview_env;
mod probe_assets;
mod probe_barrel;
mod probe_blockout;
mod probe_handson;
mod probe_kit;
mod probe_material;
mod probe_place;
mod probe_prefab;
mod probe_socket;
mod probe_user;
mod problems;
mod prompt;
mod socket_gizmo;
mod statusbar;
pub mod template_env;
mod timeline_panel;
mod view_gizmo;
mod which_key;

pub use palette::{PaletteFilter, PaletteState};

use bevy::asset::embedded_asset;
use bevy::feathers::{FeathersPlugins, dark_theme::create_dark_theme, theme::UiTheme};
use bevy::prelude::*;
use editor_core::prelude::*;

/// The editor's own panels (hierarchy, inspector) — registered through the same
/// front door as any feature crate's panels.
struct EditorUiFeature;

impl EditorFeature for EditorUiFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("editor-ui", "Editor UI")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.action(
            editor_api::actions::ActionDef::new("view.toggle-play-gizmos", "Gizmos While Playing")
                .describe("Keep feature gizmos on screen after handing the world to the game")
                .context("normal")
                .bind("space t v"),
        );
        reg.panel(PanelDecl {
            id: PanelId::new_static("hierarchy"),
            title: "Hierarchy",
            placement: Placement::Left,
            context: ContextId::new_static("hierarchy"),
            content: PanelContent::Custom,
            default_open: true,
            toggle_binding: None,
        });
        // Hierarchy focus-context actions (keymap doc): hidden from the palette
        // (panel-scoped keys, discoverable via which-key while focused).
        for (id, name, binding) in [
            ("hierarchy.down", "Row Down", "j"),
            ("hierarchy.up", "Row Up", "k"),
            ("hierarchy.top", "Top", "g g"),
            ("hierarchy.bottom", "Bottom", "shift+g"),
            ("hierarchy.select", "Select Row", "enter"),
            ("hierarchy.fold", "Fold / To Parent", "h"),
            ("hierarchy.unfold", "Unfold", "l"),
            (
                "hierarchy.reparent-in",
                "Reparent Into Sibling",
                "shift+period",
            ),
            (
                "hierarchy.reparent-out",
                "Reparent To Grandparent",
                "shift+comma",
            ),
        ] {
            reg.action(
                ActionDef::new(id, name)
                    .context("hierarchy")
                    .bind(binding)
                    .hidden(),
            );
        }
        // The way back OUT of the add-component palette, which offers every
        // reflectable component in the registry: without this you could add
        // anything and remove nothing.
        reg.action(
            ActionDef::new("component.remove", "Remove Component")
                .describe("Remove a component from the selection")
                .context("normal")
                .bind("shift+i"),
        );
        reg.panel(PanelDecl {
            id: PanelId::new_static("inspector"),
            title: "Inspector",
            placement: Placement::Right,
            context: ContextId::new_static("inspector"),
            content: PanelContent::Properties(PropertySource::Selection),
            default_open: true,
            toggle_binding: None,
        });
    }
}

/// ONE writer for `KeyCapture` (flow-audit: competing writers): the resolver stands
/// down exactly while a VISIBLE editable text field owns keyboard focus — the
/// palette input, inspector number fields, future rename boxes.
///
/// Visible is the load-bearing word. A closed palette hides its root but the
/// input entity lives on, and focus was staying on it: the resolver then stood
/// down forever, so keys went to a text box nobody could see. Every symptom of
/// that reads as "the editor stopped responding" — `i` did nothing in socket
/// mode for exactly this reason. Whoever forgets to release focus, a hidden
/// widget cannot hold the keyboard.
fn sync_key_capture(
    focus: Res<bevy::input_focus::InputFocus>,
    editable: Query<&InheritedVisibility, With<bevy::text::EditableText>>,
    mut capture: ResMut<KeyCapture>,
) {
    let captured = focus
        .get()
        .and_then(|entity| editable.get(entity).ok())
        .is_some_and(|visibility| visibility.get());
    if capture.0 != captured {
        capture.0 = captured;
    }
}

pub struct EditorUiPlugin;

impl Plugin for EditorUiPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "fonts/Inter-Regular.ttf");
        embedded_asset!(app, "fonts/Inter-Medium.ttf");
        embedded_asset!(app, "fonts/FiraCodeNerdFont-Regular.ttf");

        // User keymap overrides (rebind without recompiling). Convention: a game may
        // insert its own `KeymapPaths` before this plugin to relocate the file.
        if app.world().get_resource::<KeymapPaths>().is_none() {
            app.insert_resource(KeymapPaths {
                user: Some("editor-keymap.ron".into()),
            });
        }

        app.add_plugins(FeathersPlugins)
            .add_plugins(bevy::picking::mesh_picking::MeshPickingPlugin)
            .insert_resource({
                // Scrollbars are indicators, not focus (owner): drop the accent
                // thumb for a low-contrast neutral.
                let mut theme = UiTheme(create_dark_theme());
                // Elevation ramp (design pass): window < body < header, three
                // clear steps on a WARM graphite base. Near-black panels punched
                // holes against the (warm, light) viewport — the ramp sits
                // several value steps up so chrome and scene share one range,
                // and the slight warm tilt (R ≥ G ≥ B) ties chrome to content.
                theme.set_color("feathers.window.bg", Color::srgb(0.098, 0.096, 0.092));
                theme.set_color("feathers.pane.body.bg", Color::srgb(0.125, 0.122, 0.117));
                theme.set_color("feathers.pane.header.bg", Color::srgb(0.150, 0.147, 0.141));
                theme.set_color("feathers.scrollbar.bg", Color::NONE);
                theme.set_color(
                    "feathers.scrollbar.thumb",
                    Color::srgba(1.0, 1.0, 1.0, 0.16),
                );
                theme.set_color(
                    "feathers.scrollbar.thumb.hover",
                    Color::srgba(1.0, 1.0, 1.0, 0.32),
                );
                // Feathers text tokens joined to OUR tiers — widget text
                // (inputs, checkboxes) lands in the content tier, not white.
                theme.set_color("feathers.text.main", Color::srgb(0.76, 0.75, 0.73));
                theme.set_color("feathers.text.dim", Color::srgb(0.50, 0.49, 0.47));
                // Axis sigils at chrome volume: dusty triad in the palette's
                // range — the stock saturated R/G/B were the loudest pixels on
                // screen for the least important labels.
                theme.set_color("feathers.slider.bar", Color::srgb(0.30, 0.40, 0.56));
                theme.set_color("feathers.slider.bar.hover", Color::srgb(0.36, 0.47, 0.66));
                theme.set_color("feathers.textinput.axis.x", Color::srgb(0.71, 0.44, 0.44));
                theme.set_color("feathers.textinput.axis.y", Color::srgb(0.55, 0.64, 0.42));
                theme.set_color("feathers.textinput.axis.z", Color::srgb(0.45, 0.57, 0.75));
                theme
            });

        app.add_plugins((
            EditorCorePlugin,
            editor_scene::EditorScenePlugin,
            palette::PalettePlugin,
            editor_prefabs::EditorPrefabsPlugin,
            bevy_outliner::OutlinePlugin,
        ));
        app.add_editor_feature(EditorUiFeature);
        app.add_editor_feature(material_editor::MaterialEditorFeature);
        app.add_editor_feature(timeline_panel::TimelinePanelFeature);
        app.add_editor_feature(problems::ProblemsFeature);
        app.add_editor_feature(assets::AssetsFeature);
        app.add_observer(socket_gizmo::on_socket_added);
        app.add_observer(socket_gizmo::on_socket_removed);

        app.init_resource::<which_key::WhichKey>();
        app.init_resource::<probe_user::UserProbe>();
        app.init_resource::<probe_kit::KitProbe>();
        app.init_resource::<probe_barrel::BarrelProbe>();
        app.init_resource::<probe_blockout::BlockoutProbe>();
        app.init_resource::<probe_place::PlaceProbe>();
        app.add_systems(Startup, marquee::spawn_marquee);
        app.add_systems(
            Update,
            marquee::sync_marquee.in_set(editor_core::EditorSet::Sync),
        );
        app.init_resource::<probe_material::MaterialProbe>();
        app.init_resource::<probe_socket::SocketProbe>();
        app.init_resource::<probe_handson::HandsonProbe>();
        app.init_resource::<grid::GridVisible>();
        app.init_resource::<socket_gizmo::SocketGizmoAssets>();
        app.init_resource::<open_indicator::DimAssets>();
        app.init_resource::<inspector::InspectorReveal>();
        app.init_resource::<palette_preview::PreviewSubject>();
        app.init_resource::<template_env::TemplateEnvironment>();
        app.init_resource::<hierarchy::HierarchyDrag>();
        app.init_resource::<material_editor::MaterialEditorState>();
        app.init_resource::<material_editor::MaterialHistory>();
        app.init_resource::<material_editor::PendingSeeds>();
        app.init_resource::<material_editor::RenameTarget>();
        app.init_resource::<material_editor::PendingTextureSlot>();
        app.init_resource::<timeline_panel::TimelinePanelState>();
        app.init_resource::<inspector::PendingFieldKeys>();
        app.init_resource::<problems::ProblemsState>();
        app.init_resource::<hierarchy::HierarchyState>();
        app.init_resource::<assets::AssetsState>();
        app.init_resource::<assets::AssetReveal>();
        app.init_resource::<probe_assets::AssetsProbe>();
        app.init_resource::<inspector::InspectorModel>();
        app.init_resource::<inspector::InspectorGroups>();
        app.insert_resource(inspector::default_overrides());
        if !app.is_plugin_added::<bevy::input_focus::tab_navigation::TabNavigationPlugin>() {
            app.add_plugins(bevy::input_focus::tab_navigation::TabNavigationPlugin);
        }
        app.add_systems(
            Update,
            hierarchy::handle_hierarchy_actions.in_set(editor_core::EditorSet::Tools),
        );
        app.init_resource::<statusbar::StatusFlash>();
        app.add_systems(
            Update,
            (
                dock::track_pointer_over_chrome,
                dock::sync_dock_chrome,
                dock::style_scrollbars,
                hierarchy::watch_hierarchy_inputs,
                hierarchy::watch_hierarchy_window,
                hierarchy::rebuild_hierarchy,
                hierarchy::scroll_cursor_into_view,
                inspector::watch_inspector_inputs,
                inspector::collect_inspector,
                inspector::render_inspector,
                inspector::stamp_tab_indices,
                inspector::probe_inspector.run_if(|| std::env::var("INSPECTOR_PROBE").is_ok()),
                sync_key_capture,
                ghost::apply_ghost_material,
                outline::ensure_outline_camera,
                outline::sync_selection_outlines,
                statusbar::collect_io_feedback,
                which_key::compute_which_key,
                statusbar::update_statusbar,
                which_key::rebuild_which_key,
            )
                .chain()
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            (
                prompt::attach_prompt_input,
                prompt::sync_prompt,
                prompt::close_prompt_on_escape,
                open_indicator::sync_open_pill,
                open_indicator::dim_outsiders,
                appear::animate_appearing,
                grid::handle_grid_actions,
                grid::sync_grid,
                socket_gizmo::sync_socket_gizmos,
                inspector::reveal_section,
                (
                    hierarchy::perform_hierarchy_drop,
                    assets::watch_asset_sources,
                    assets::collect_asset_actions,
                    assets::perform_asset_activate,
                    assets::open_palette_from_assets,
                    assets::perform_reveal,
                    assets::rebuild_assets,
                    confirm::sync_confirm,
                    palette_preview::sync_preview_content,
                    template_env::sync_template_environment,
                    palette_preview::frame_preview,
                )
                    .chain(),
                palette_preview::contain_preview_content,
                palette_preview::turn_preview,
                probe_prefab::probe_prefab.run_if(|| std::env::var("PREFAB_PROBE").is_ok()),
                probe_user::probe_user.run_if(|| std::env::var("USER_PROBE").is_ok()),
                probe_user::log_actions.run_if(|| std::env::var("USER_PROBE").is_ok()),
                probe_kit::probe_kit.run_if(|| std::env::var("KIT_PROBE").is_ok()),
                probe_barrel::probe_barrel.run_if(|| std::env::var("BARREL_PROBE").is_ok()),
                probe_material::probe_material.run_if(|| std::env::var("MATERIAL_PROBE").is_ok()),
                probe_handson::probe_handson.run_if(|| std::env::var("HANDSON_PROBE").is_ok()),
            )
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            (
                timeline_panel::handle_timeline_actions,
                timeline_panel::commit_timeline_event,
                material_editor::collect_editor_actions,
                material_editor::collect_rename,
                material_editor::handle_material_library_verbs,
                material_editor::apply_rename,
                material_editor::apply_material_history,
                inspector::perform_field_keys,
                timeline_panel::sync_timeline_rows,
                timeline_panel::sync_timeline_cursor,
                material_editor::sync_editor_ui,
                material_editor::seed_slider_values,
                material_editor::sync_preview,
                material_editor::sync_readouts,
                problems::collect_problem_actions,
                problems::sync_problems_ui,
            )
                .chain()
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Startup,
            (
                style::load_ui_fonts,
                ghost::init_ghost_material,
                statusbar::spawn_statusbar,
                which_key::spawn_which_key,
                prompt::spawn_prompt,
                confirm::spawn_confirm,
                grid::spawn_grid,
                preview_env::setup_preview_environment,
                template_env::setup_template_skies,
                palette_preview::setup_preview_rig,
                material_editor::setup_material_preview,
                material_editor::spawn_editor_root,
                problems::spawn_problems_root,
                dock::spawn_docks,
                open_indicator::spawn_open_pill,
                dock::attach_scrollbars,
                view_gizmo::spawn_view_gizmo,
                timeline_panel::spawn_timeline_panel,
            )
                .chain(),
        );
        // Its own registration: the Sync tuple above is at the ECS arity limit.
        app.add_systems(
            Update,
            light_gizmo::draw_light_gizmos.in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            view_gizmo::sync_view_gizmo.in_set(editor_core::EditorSet::Sync),
        );
        app.init_resource::<feature_gizmos::PickProxyMesh>();
        app.init_resource::<feature_gizmos::GizmosWhilePlaying>();
        app.add_systems(
            Update,
            (
                feature_gizmos::toggle_gizmos_while_playing,
                feature_gizmos::draw_feature_gizmos,
                feature_gizmos::attach_gizmo_pick_targets,
            )
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            probe_socket::probe_socket
                .run_if(|| std::env::var("SOCKET_PROBE").is_ok())
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            probe_assets::probe_assets
                .run_if(|| std::env::var("ASSETS_PROBE").is_ok())
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            probe_place::probe_place
                .run_if(|| std::env::var("PLACE_PROBE").is_ok())
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            probe_blockout::count_timeline_events
                .run_if(|| std::env::var("BLOCKOUT_PROBE").is_ok())
                .in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(
            Update,
            probe_blockout::probe_blockout
                .run_if(|| std::env::var("BLOCKOUT_PROBE").is_ok())
                .in_set(editor_core::EditorSet::Sync),
        );
    }
}
