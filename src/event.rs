use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, MouseEvent};

const TICK_RATE_MS: u64 = 250;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    #[allow(dead_code)]
    Resize(u16, u16),
    Tick,
}

/// Poll for crossterm events with a tick-rate timeout.
/// Returns Tick when no event arrives within the timeout.
pub fn poll_event() -> Result<AppEvent> {
    if event::poll(Duration::from_millis(TICK_RATE_MS))? {
        match event::read()? {
            Event::Key(k) => Ok(AppEvent::Key(k)),
            Event::Mouse(m) => Ok(AppEvent::Mouse(m)),
            Event::Resize(w, h) => Ok(AppEvent::Resize(w, h)),
            _ => Ok(AppEvent::Tick),
        }
    } else {
        Ok(AppEvent::Tick)
    }
}
