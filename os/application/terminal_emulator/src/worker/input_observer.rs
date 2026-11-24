use core::cell::RefCell;

use alloc::rc::Rc;
use globals::hotkeys::{HKEY_CLOSE_TAB, HKEY_NEW_TAB, HKEY_NEXT_TAB, HKEY_PREV_TAB, HKEY_TOGGLE_TERMINAL_WINDOW};
use ime::{DecodedKey, KeyCode, KeyEvent, KeyboardInput};
use stream::event_to_u16;
use terminal_lib::TerminalMode;
use terminal_lib::session::{CtlRecord, WaitState};

use crate::{
    event_handler::{Event, EventHandler},
    sessions::SessionMux,
    worker::canonical::CanonicalAction,
};

use super::worker::Worker;

pub struct InputObserver {
    event_handler: Rc<RefCell<EventHandler>>,
    mode: TerminalMode,
    /// Shared session multiplexer: owns per-session endpoints, canonical
    /// editors, and routes input to the active session's foreground app.
    mux: Rc<RefCell<SessionMux>>,
}

impl InputObserver {
    pub fn new(event_handler: Rc<RefCell<EventHandler>>, mux: Rc<RefCell<SessionMux>>) -> Self {
        Self {
            event_handler,
            mode: TerminalMode::Raw,
            mux,
        }
    }

    /// Resolve the current foreground input mode from the control record.
    /// `None` means no foreground app is currently waiting for input.
    fn resolve_mode(&self, record: Option<CtlRecord>) -> Option<TerminalMode> {
        match record {
            Some(record) if record.wait == WaitState::Waiting => Some(record.mode),
            _ => None,
        }
    }

    /// Deliver a decoded input buffer to the active session's foreground app.
    fn deliver(&mut self, buffer: &[u8]) {
        self.mux.borrow_mut().deliver(buffer);
    }
}

impl Worker for InputObserver {
    fn run(&mut self) {
        self.mux.borrow_mut().ensure_active_in_writer();

        let Ok(Some(KeyboardInput { raw: key_event, decoded })) = ime::read_event(false) else {
            return;
        };

        let record = self.mux.borrow().active_ctl();
        let use_pipe = record.is_some();
        let mode = self.resolve_mode(record);

        if let Some(mode) = mode {
            self.mode = mode;
        }

        // Process key event into decoded key (unicode char or raw keycode)
        let Some(decoded_key) = decoded else {
            // This returns none if the key event was a key release
            // In this case, we only process the byte if the terminal is in raw mode
            if use_pipe && self.mode == TerminalMode::Raw {
                self.deliver_raw(key_event);
            }

            return;
        };

        // Handle reserved keys (e.g. hotkeys)
        let Some(decoded_key) = self.try_intercept_reserved_key(decoded_key) else {
            return;
        };

        // Buffer the decoded key based on the terminal input mode. Canonical
        // mode edits and echoes through the active session's own state.
        match mode {
            Some(TerminalMode::Canonical) => {
                if let Some(action) = Self::canonical_action(decoded_key) {
                    self.mux.borrow_mut().handle_canonical(action);
                }
            }
            Some(TerminalMode::Fluid) => {
                self.deliver(&ime::wire::encode_key(decoded_key).to_ne_bytes());
            }
            Some(TerminalMode::Raw) => {
                self.deliver_raw(key_event);
            }
            None => {}
        }
    }
}

impl InputObserver {
    /// Check if the pressed key corresponds to a command.
    ///
    /// If this is the case, queue it. Else, this is just input for the applications.
    fn try_intercept_reserved_key(&self, key: DecodedKey) -> Option<DecodedKey> {
        self.event_handler.borrow_mut().trigger(match key {
            DecodedKey::RawKey(HKEY_TOGGLE_TERMINAL_WINDOW) => Event::EnterGuiMode,
            DecodedKey::RawKey(HKEY_NEW_TAB) => Event::NewTab,
            DecodedKey::RawKey(HKEY_CLOSE_TAB) => Event::CloseActiveTab,
            DecodedKey::RawKey(HKEY_NEXT_TAB) => Event::NextTab,
            DecodedKey::RawKey(HKEY_PREV_TAB) => Event::PrevTab,
            // no event, just normal input
            key => return Some(key),
        });
        None
    }

    fn deliver_raw(&mut self, event: KeyEvent) {
        self.deliver(&u32::from(event_to_u16(event)).to_ne_bytes());
    }

    /// Map a decoded key to a canonical-mode edit action, or `None` when the key
    /// has no canonical effect.
    fn canonical_action(key: DecodedKey) -> Option<CanonicalAction> {
        match key {
            DecodedKey::RawKey(KeyCode::ArrowLeft) => Some(CanonicalAction::Left),
            DecodedKey::RawKey(KeyCode::ArrowRight) => Some(CanonicalAction::Right),
            DecodedKey::RawKey(KeyCode::Home) => Some(CanonicalAction::Home),
            DecodedKey::RawKey(KeyCode::End) => Some(CanonicalAction::End),
            DecodedKey::RawKey(_) => None,
            DecodedKey::Unicode('\x1B') => None,
            DecodedKey::Unicode('\n') => Some(CanonicalAction::Submit),
            DecodedKey::Unicode('\x08') => Some(CanonicalAction::Backspace),
            DecodedKey::Unicode('\x7F') => Some(CanonicalAction::Delete),
            DecodedKey::Unicode(ch) => Some(CanonicalAction::Insert(ch)),
        }
    }
}
