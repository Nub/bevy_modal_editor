use bevy::prelude::*;
use std::collections::VecDeque;

/// Maximum number of undo/redo entries to keep
const MAX_HISTORY: usize = 100;

/// Trait for reversible commands
pub trait EditorCommand: Send + Sync {
    fn execute(&self, world: &mut World);
    fn undo(&self, world: &mut World);
    fn description(&self) -> &str;
}

/// Resource to manage undo/redo history
#[derive(Resource)]
pub struct CommandHistory {
    undo_stack: VecDeque<Box<dyn EditorCommand>>,
    redo_stack: VecDeque<Box<dyn EditorCommand>>,
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self {
            undo_stack: VecDeque::with_capacity(MAX_HISTORY),
            redo_stack: VecDeque::with_capacity(MAX_HISTORY),
        }
    }
}

impl CommandHistory {
    pub fn push(&mut self, command: Box<dyn EditorCommand>) {
        // Clear redo stack when a new command is pushed
        self.redo_stack.clear();

        // Add to undo stack
        if self.undo_stack.len() >= MAX_HISTORY {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(command);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_description(&self) -> Option<&str> {
        self.undo_stack.back().map(|c| c.description())
    }

    pub fn redo_description(&self) -> Option<&str> {
        self.redo_stack.back().map(|c| c.description())
    }
}

/// Event to trigger undo
#[derive(Message)]
pub struct UndoEvent;

/// Event to trigger redo
#[derive(Message)]
pub struct RedoEvent;

pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandHistory>()
            .add_message::<UndoEvent>()
            .add_message::<RedoEvent>()
            .add_systems(Update, handle_undo_redo_input);
    }
}

fn handle_undo_redo_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut undo_events: MessageWriter<UndoEvent>,
    mut redo_events: MessageWriter<RedoEvent>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    if ctrl && keyboard.just_pressed(KeyCode::KeyZ) {
        if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
            redo_events.write(RedoEvent);
        } else {
            undo_events.write(UndoEvent);
        }
    }

    // Alternative: Ctrl+Y for redo
    if ctrl && keyboard.just_pressed(KeyCode::KeyY) {
        redo_events.write(RedoEvent);
    }
}
