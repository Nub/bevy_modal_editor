//! Panel focus (RFC §9, spec §"Modes"): the kernel owns WHICH panel is focused and
//! what that means for the keymap — a focused panel is a *focus target with its own
//! keymap layer* that replaces the mode layer (j/k belong to the tree, not the
//! viewport). Docking, chrome, and rendering live in `editor_ui`; the kernel never
//! draws.
//!
//! Navigation is spatial (keymap doc: `Ctrl-h/j/k/l` focus panel left/down/up/right,
//! viewport at the center): Left dock ← viewport → Right dock, Bottom dock below;
//! repeated up/down steps through a dock's stack. Escape returns focus to the
//! viewport (handled with the other escape layers in the resolver — one layer per
//! press).

use bevy::prelude::*;
use editor_api::prelude::*;
use std::collections::HashMap;

use crate::resolver::EditorState;

/// All registered panels, in registration order (which is also dock-stack order).
#[derive(Resource, Default)]
pub struct PanelCatalog {
    pub panels: Vec<PanelDecl>,
}

impl PanelCatalog {
    pub fn get(&self, id: &PanelId) -> Option<&PanelDecl> {
        self.panels.iter().find(|p| &p.id == id)
    }
    pub fn in_placement(&self, placement: Placement) -> impl Iterator<Item = &PanelDecl> {
        self.panels.iter().filter(move |p| p.placement == placement)
    }
}

/// Per-panel open state, toggled via the synthesized `panel.toggle.<id>` actions.
#[derive(Resource, Default)]
pub struct PanelStates(pub HashMap<PanelId, bool>);

impl PanelStates {
    pub fn open(&self, id: &PanelId) -> bool {
        self.0.get(id).copied().unwrap_or(false)
    }
}

/// The focused panel; `None` = the viewport owns focus (mode context active).
#[derive(Resource, Default)]
pub struct PanelFocus(pub Option<PanelId>);

enum Dir {
    Left,
    Down,
    Up,
    Right,
}

pub(crate) fn handle_panel_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    catalog: Res<PanelCatalog>,
    mut states: ResMut<PanelStates>,
    mut focus: ResMut<PanelFocus>,
) {
    // Editor deactivation (F12, play) always releases panel focus — coming back
    // must land in the viewport, never inside a hidden panel (flow-audit).
    if !state.active {
        if focus.0.is_some() {
            focus.0 = None;
        }
        return;
    }
    for invoked in reader.read() {
        if let Some(id) = invoked.action.as_str().strip_prefix("panel.toggle.") {
            let id = PanelId::new(id.to_string());
            if catalog.get(&id).is_some() {
                let open = !states.open(&id);
                states.0.insert(id.clone(), open);
                if !open && focus.0.as_ref() == Some(&id) {
                    focus.0 = None;
                }
            }
            continue;
        }
        let dir = match invoked.action.as_str() {
            "panel.focus-left" => Dir::Left,
            "panel.focus-down" => Dir::Down,
            "panel.focus-up" => Dir::Up,
            "panel.focus-right" => Dir::Right,
            _ => continue,
        };
        step_focus(&mut focus, &catalog, &states, dir);
    }
}

/// First OPEN panel in a placement (dock-stack order).
fn first_open(
    catalog: &PanelCatalog,
    states: &PanelStates,
    placement: Placement,
) -> Option<PanelId> {
    catalog.in_placement(placement).map(|p| &p.id).find(|id| states.open(id)).cloned()
}

/// Neighbor within the focused panel's own dock stack (+1 = down, -1 = up).
fn dock_neighbor(
    catalog: &PanelCatalog,
    states: &PanelStates,
    current: &PanelId,
    delta: isize,
) -> Option<PanelId> {
    let decl = catalog.get(current)?;
    let stack: Vec<&PanelId> = catalog
        .in_placement(decl.placement)
        .map(|p| &p.id)
        .filter(|id| states.open(id))
        .collect();
    let index = stack.iter().position(|id| *id == current)? as isize;
    stack.get(usize::try_from(index + delta).ok()?).copied().cloned()
}

fn step_focus(focus: &mut PanelFocus, catalog: &PanelCatalog, states: &PanelStates, dir: Dir) {
    let next = match &focus.0 {
        // Viewport is the center: h/l reach the side docks, j the bottom dock.
        None => match dir {
            Dir::Left => first_open(catalog, states, Placement::Left),
            Dir::Right => first_open(catalog, states, Placement::Right),
            Dir::Down => first_open(catalog, states, Placement::Bottom),
            Dir::Up => None,
        },
        Some(current) => {
            let Some(decl) = catalog.get(current) else {
                focus.0 = None;
                return;
            };
            match (decl.placement, dir) {
                // Toward the center: back to the viewport.
                (Placement::Left, Dir::Right)
                | (Placement::Right, Dir::Left)
                | (Placement::Bottom, Dir::Up) => None,
                // Within the dock stack; falling off the bottom reaches the bottom dock.
                (placement, Dir::Down) => dock_neighbor(catalog, states, current, 1)
                    .or_else(|| {
                        (placement != Placement::Bottom)
                            .then(|| first_open(catalog, states, Placement::Bottom))
                            .flatten()
                    })
                    .or_else(|| Some(current.clone())),
                (_, Dir::Up) => {
                    Some(dock_neighbor(catalog, states, current, -1).unwrap_or_else(|| current.clone()))
                }
                // No panel further outward: stay put.
                _ => Some(current.clone()),
            }
        }
    };
    focus.0 = next;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::{active_contexts, EditorState, OverlayContext};
    use crate::modes::CurrentMode;
    use crate::EditorCorePlugin;

    struct PanelFeature;
    impl EditorFeature for PanelFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("panel-test", "Panel Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.panel(PanelDecl {
                id: PanelId::new_static("hierarchy-test"),
                title: "Hierarchy",
                placement: Placement::Left,
                context: ContextId::new_static("hierarchy-test"),
                content: PanelContent::Custom,
                default_open: true,
            })
            .panel(PanelDecl {
                id: PanelId::new_static("inspector-test"),
                title: "Inspector",
                placement: Placement::Right,
                context: ContextId::new_static("inspector-test"),
                content: PanelContent::Properties(PropertySource::Selection),
                default_open: true,
            });
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(PanelFeature);
        app.init_resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    fn invoke(app: &mut App, action: &str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new(action.to_string()),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
    }

    fn focused(app: &App) -> Option<String> {
        app.world().resource::<PanelFocus>().0.as_ref().map(|id| id.to_string())
    }

    // C1: registration populates the catalog; focus is spatial; the focused panel's
    // context replaces the mode layer; Escape (via the resolver conventions) and
    // editor deactivation return focus to the viewport.
    #[test]
    fn panel_focus_and_contexts() {
        let mut app = test_app();
        assert_eq!(app.world().resource::<PanelCatalog>().panels.len(), 2);

        invoke(&mut app, "panel.focus-left");
        assert_eq!(focused(&app).as_deref(), Some("hierarchy-test"));

        // Focused panel context replaces the mode layer.
        let world = app.world();
        let contexts = active_contexts(
            world.resource::<EditorState>(),
            world.resource::<CurrentMode>(),
            world.resource::<OverlayContext>(),
            world.resource::<PanelFocus>(),
            world.resource::<PanelCatalog>(),
        );
        assert!(contexts.contains(&ContextId::new_static("hierarchy-test")));
        assert!(!contexts.iter().any(|c| c.as_str() == "normal"));

        // Right from a Left panel = viewport; right again = the Right dock.
        invoke(&mut app, "panel.focus-right");
        assert_eq!(focused(&app), None);
        invoke(&mut app, "panel.focus-right");
        assert_eq!(focused(&app).as_deref(), Some("inspector-test"));

        // Escape walks home: panel focus is the first layer.
        invoke(&mut app, "core.escape-home");
        assert_eq!(focused(&app), None);

        // Closing a focused panel releases focus.
        invoke(&mut app, "panel.focus-left");
        invoke(&mut app, "panel.toggle.hierarchy-test");
        assert_eq!(focused(&app), None);
        assert!(!app.world().resource::<PanelStates>().open(&PanelId::new_static("hierarchy-test")));

        // Editor deactivation always lands focus back in the viewport.
        invoke(&mut app, "panel.focus-right");
        assert_eq!(focused(&app).as_deref(), Some("inspector-test"));
        app.world_mut().resource_mut::<EditorState>().active = false;
        app.update();
        assert_eq!(focused(&app), None);
    }
}
