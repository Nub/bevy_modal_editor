//! Editing the PREFAB, not an instance of it (spec §6 prefab edit mode).
//!
//! Owner: "prefabs when edited get their own scene/origin. When placed, a
//! prefab needs to know it's an instance and not the prefab itself — editing a
//! prefab and a prefab instance are not the same."
//!
//! So there are two verbs on purpose, and they are never the same key:
//!
//! - **Enter** on an instance opens it IN PLACE. You are editing THIS copy; the
//!   level stays around it and edits become overrides. That is the flow the
//!   owner asked for in 2026-08-02 after v1's prefab UX was "not clear when
//!   editing a prefab or a scene", and it stays exactly as it was.
//! - **`space e`** opens the TEMPLATE in a scene of its own, at its own origin,
//!   with the level parked. Changes here are the prefab itself, so they reach
//!   every instance.
//!
//! The level is not "hidden" — it is CAPTURED and restored, the same snapshot
//! machinery scene save/load uses. That matters because a world swap is where
//! v1 lost people's work: the level exists as one value the whole time, and
//! coming back is applying it, not rebuilding it.
//!
//! While the template is open, scene I/O and play are refused rather than
//! quietly operating on the wrong world — saving a template over your level is
//! the exact failure this mode could otherwise introduce.

use crate::{PrefabInstance, PrefabLibrary};
use bevy::prelude::*;
use editor_core::prelude::*;
use editor_scene::SceneSnapshot;
use uuid::Uuid;

/// The level, parked while its prefab is being edited.
#[derive(Resource, Default)]
pub struct TemplateEdit {
    pub prefab: Option<Uuid>,
    pub name: String,
    /// `None` while editing a level: this IS the level, held as a value.
    level: Option<SceneSnapshot>,
}

impl TemplateEdit {
    pub fn active(&self) -> bool {
        self.prefab.is_some()
    }
}

#[derive(Resource, Default)]
pub(crate) struct TemplateRequests {
    pub open: bool,
    pub close: bool,
}

pub(crate) fn collect_template_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    mut requests: ResMut<TemplateRequests>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "prefab.edit-template" => requests.open = true,
            "prefab.close-template" => requests.close = true,
            _ => {}
        }
    }
}

pub(crate) fn perform_template_actions(world: &mut World) {
    let requests = std::mem::take(&mut *world.resource_mut::<TemplateRequests>());
    if requests.open && !world.resource::<TemplateEdit>().active() {
        open_template(world);
    }
    if requests.close && world.resource::<TemplateEdit>().active() {
        close_template(world);
    }
}

/// Which prefab the selection means: an instance names its own prefab.
fn selected_prefab(world: &mut World) -> Option<Uuid> {
    let mut query = world.query_filtered::<&PrefabInstance, With<Selected>>();
    if let Some(instance) = query.iter(world).next() {
        return Some(instance.0);
    }
    // A selected MEMBER of an instance means the instance it belongs to — you
    // should not have to reselect the root to edit the thing you are looking at.
    let selected: Vec<Entity> = world
        .query_filtered::<Entity, With<Selected>>()
        .iter(world)
        .collect();
    for entity in selected {
        let mut current = entity;
        loop {
            if let Some(instance) = world.get::<PrefabInstance>(current) {
                return Some(instance.0);
            }
            match world.get::<ChildOf>(current) {
                Some(parent) => current = parent.parent(),
                None => break,
            }
        }
    }
    None
}

fn open_template(world: &mut World) {
    let Some(prefab) = selected_prefab(world) else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select an instance to edit its prefab".into(),
            success: false,
        });
        return;
    };
    // The snapshot is not `Clone` (a record holds boxed reflected values), so
    // rebuild it from its parts — the same way stamping reads a template.
    let Some((template, name)) =
        world
            .resource::<PrefabLibrary>()
            .prefabs
            .get(&prefab)
            .map(|def| {
                (
                    editor_scene::snapshot_from_parts(
                        def.template
                            .records()
                            .map(|(id, parent, components)| {
                                (
                                    id,
                                    parent,
                                    components.iter().map(|c| c.to_dynamic()).collect(),
                                )
                            })
                            .collect(),
                    ),
                    def.name.clone(),
                )
            })
    else {
        return;
    };
    // Park the LEVEL as a value. Restoring is then applying a snapshot rather
    // than reconstructing a world, which is the whole reason this is safe.
    let level = editor_scene::capture_scene(world);
    editor_scene::apply_scene(world, &template, true);
    {
        let mut edit = world.resource_mut::<TemplateEdit>();
        edit.prefab = Some(prefab);
        edit.name = name.clone();
        edit.level = Some(level);
    }
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("editing the PREFAB {name} \u{b7} \u{238b} back to the level"),
        success: true,
    });
}

fn close_template(world: &mut World) {
    let (prefab, name, level) = {
        let mut edit = world.resource_mut::<TemplateEdit>();
        (
            edit.prefab.take(),
            std::mem::take(&mut edit.name),
            edit.level.take(),
        )
    };
    let (Some(prefab), Some(level)) = (prefab, level) else {
        return;
    };
    // What is in the world IS the new template — captured in its own frame,
    // because the template scene has its own origin.
    let edited = editor_scene::capture_scene(world);
    let changed = {
        let mut library = world.resource_mut::<PrefabLibrary>();
        match library.prefabs.get_mut(&prefab) {
            Some(def) => {
                def.template = edited;
                true
            }
            None => false,
        }
    };
    if changed {
        // The generation bump is what restamps every instance (D5 propagation).
        world.resource_mut::<PrefabLibrary>().generation += 1;
        // AND IT GOES TO DISK. Every other path that edits a prefab — group,
        // make-variant, apply-to-prefab — writes the file; this one bumped the
        // generation, updated instances on screen, said "every instance
        // follows", and discarded the work at the next launch. A tool that
        // silently eats an edit is worse than one that refuses it.
        let def = world
            .resource::<PrefabLibrary>()
            .prefabs
            .get(&prefab)
            .map(|def| crate::PrefabDef {
                kit: def.kit.clone(),
                id: def.id,
                name: def.name.clone(),
                template: editor_scene::snapshot_from_parts(
                    def.template
                        .records()
                        .map(|(id, parent, components)| {
                            (
                                id,
                                parent,
                                components.iter().map(|c| c.to_dynamic()).collect(),
                            )
                        })
                        .collect(),
                ),
            });
        if let Some(def) = def {
            crate::authoring::save_prefab_public(world, &def);
        }
    }
    editor_scene::apply_scene(world, &level, true);
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("{name} updated \u{b7} every instance follows"),
        success: true,
    });
}

/// While a template is open the level is not in the world. Saving, loading or
/// playing would all operate on the wrong thing, so they are refused OUT LOUD
/// rather than silently doing the wrong one.
pub(crate) fn guard_scene_io_while_editing_template(
    edit: Res<TemplateEdit>,
    mut reader: MessageReader<ActionInvoked>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    if !edit.active() {
        return;
    }
    for invoked in reader.read() {
        if matches!(
            invoked.action.as_str(),
            "scene.save" | "scene.open" | "editor.play" | "editor.reset"
        ) {
            feedback.write(editor_scene::SceneIoFeedback {
                message: format!(
                    "editing the prefab {} — \u{238b} back to the level first",
                    edit.name
                ),
                success: false,
            });
        }
    }
}

/// The `template` keymap layer is live exactly while the template is open.
///
/// LAYERED, not exclusive: editing a prefab is ordinary editing — move, rotate,
/// the palette, undo all mean what they always mean. The layer exists to give
/// Escape a different job (back to the level, saving the prefab) than the one it
/// has in the level (clear the selection).
pub(crate) fn hold_template_layer(
    edit: Res<TemplateEdit>,
    mut overlay: ResMut<editor_core::resolver::OverlayContext>,
) {
    let context = editor_api::prelude::ContextId::new_static("template");
    if edit.active() {
        if overlay.context.as_ref() != Some(&context) {
            overlay.set_layer(context);
        }
    } else if overlay.context.as_ref() == Some(&context) {
        overlay.clear();
    }
}
