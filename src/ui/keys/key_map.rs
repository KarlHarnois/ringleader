use crossterm::event::KeyEvent;

use super::action_gate::KeyAction;

pub trait KeyMap {
    fn action_for(&self, key: &KeyEvent) -> KeyAction;

    fn open_in_browser(&self) -> KeyAction;
}
