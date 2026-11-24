use alloc::rc::Rc;
use time::systime;

use super::worker::Worker;
use crate::terminal::lfb_terminal::LFBTerminal;

const UPDATE_INTERVAL: i64 = 1000;
const LAYOUT_POLL_INTERVAL: i64 = 50;

pub struct StatusBar {
    terminal: Rc<LFBTerminal>,
    last_tick: i64,
    last_poll: i64,
    last_layout: Option<ime::Layout>,
}

impl StatusBar {
    pub const fn new(terminal: Rc<LFBTerminal>) -> Self {
        Self {
            terminal,
            last_tick: -UPDATE_INTERVAL,
            last_poll: -LAYOUT_POLL_INTERVAL,
            last_layout: None,
        }
    }
}

impl Worker for StatusBar {
    fn run(&mut self) {
        let systime = systime().num_milliseconds();
        if systime < self.last_poll + LAYOUT_POLL_INTERVAL {
            return;
        }
        self.last_poll = systime;
        let layout = ime::get_layout().ok();
        if layout != self.last_layout || systime >= self.last_tick + UPDATE_INTERVAL {
            self.last_tick = systime;
            self.last_layout = layout;
            LFBTerminal::draw_status_bar(&mut self.terminal.display.lock(), layout);
        }
    }
}
