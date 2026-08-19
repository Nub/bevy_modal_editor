//! Inline name prompt (prefab UX redesign): `g` opens a small floating field
//! above the statusbar — type a name, Enter groups the selection in place,
//! Escape cancels. One prompt, reusable shape for future rename flows.

use bevy::feathers::controls::{FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;
use editor_prefabs::authoring::{GroupCommit, GroupPrompt, PromptPurpose};

use crate::style::{self, UiFonts};

#[derive(Component, Default, Clone)]
pub(crate) struct PromptRoot;
#[derive(Component, Default, Clone)]
pub(crate) struct PromptInput;
#[derive(Component, Default, Clone)]
pub(crate) struct PromptTitle;

pub(crate) fn spawn_prompt(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
) {
    let ui = settings.ui.clone();
    commands
        .spawn((
            PromptRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(style::BAR_HEIGHT + style::space::S),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(210),
            Visibility::Hidden,
            bevy::ui::UiTransform::IDENTITY,
            crate::appear::FloatingSurface::default(),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(style::space::XS),
                        padding: UiRect::all(px(style::space::S)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(style::radius::L)),
                        width: px(320.0),
                        ..default()
                    },
                    ThemeBackgroundColor(tokens::WINDOW_BG),
                    BorderColor::all(style::HAIRLINE),
                    style::floating_shadow(),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        PromptTitle,
                        Text::new("GROUP INTO PREFAB — name it"),
                        style::sans_medium(&fonts, ui.font_size_xs),
                        TextColor(style::color::TEXT_DIM),
                    ));
                });
        });
}

/// The input is spawned once the panel exists (bsn widgets under a plain node).
pub(crate) fn attach_prompt_input(
    root: Query<&Children, With<PromptRoot>>,
    inputs: Query<(), With<PromptInput>>,
    mut commands: Commands,
) {
    if !inputs.is_empty() {
        return;
    }
    let Ok(children) = root.single() else { return };
    let Some(panel) = children.iter().next() else {
        return;
    };
    let input = commands
        .spawn_scene(bsn! {
            @FeathersTextInputContainer
            Node { flex_grow: 1.0 }
            Children [
                (
                    @FeathersTextInput
                    PromptInput
                    bevy::ui_widgets::SelectAllOnFocus
                    on(prompt_keys)
                )
            ]
        })
        .id();
    commands.entity(input).insert(ChildOf(panel));
}

/// Open/close driven by the prefabs crate's `GroupPrompt` resource.
pub(crate) fn sync_prompt(
    prompt: Res<GroupPrompt>,
    mut root: Query<&mut Visibility, With<PromptRoot>>,
    mut title: Query<&mut Text, With<PromptTitle>>,
    input: Query<Entity, With<PromptInput>>,
    mut editable: Query<&mut bevy::text::EditableText>,
    mut focus: ResMut<InputFocus>,
    mut was_open: Local<bool>,
) {
    if prompt.open == *was_open {
        return;
    }
    *was_open = prompt.open;
    for mut visibility in &mut root {
        *visibility = if prompt.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if prompt.open {
        for mut text in &mut title {
            text.0 = match prompt.purpose {
                PromptPurpose::Group => "GROUP INTO PREFAB — name it".into(),
                PromptPurpose::Variant => "MAKE VARIANT — name it".into(),
                PromptPurpose::Kit => "KIT — name it (empty clears)".into(),
                PromptPurpose::Fill => "FILL RUN — how many pieces?".into(),
                PromptPurpose::RenameMaterial => "RENAME MATERIAL".into(),
            };
        }
        if let Ok(entity) = input.single() {
            if let Ok(mut text) = editable.get_mut(entity) {
                text.clear();
            }
            focus.set(entity, FocusCause::Navigated);
        }
    } else if focus
        .get()
        .is_some_and(|f| input.single().is_ok_and(|i| i == f))
    {
        focus.clear();
    }
}

/// Escape backs out of the prompt (the pierced-capture escape-home path).
pub(crate) fn close_prompt_on_escape(
    mut reader: MessageReader<ActionInvoked>,
    mut prompt: ResMut<GroupPrompt>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "core.escape-home" && prompt.open {
            prompt.open = false;
        }
    }
}

fn prompt_keys(
    mut event: On<FocusedInput<KeyboardInput>>,
    text: Query<&bevy::text::EditableText, With<PromptInput>>,
    mut prompt: ResMut<GroupPrompt>,
    mut commit: ResMut<GroupCommit>,
) {
    if !prompt.open || event.input.state != ButtonState::Pressed {
        return;
    }
    if event.input.key_code == KeyCode::Enter {
        if let Ok(editable) = text.get(event.event_target()) {
            commit.0 = Some(editable.value().to_string());
        }
        prompt.open = false;
        event.propagate(false);
    }
}
