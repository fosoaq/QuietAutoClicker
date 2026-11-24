//! Mouse and keyboard injection through CoreGraphics events.
//!
//! Mirrors the small part of enigo's API the engine uses. enigo itself is not
//! used here: it resolves characters through the active keyboard layout on the
//! calling thread (main-thread-only API, and wrong under non-Latin layouts) and
//! sleeps 20 ms per sent event when dropped, which would stall quitting.

use std::time::{Duration, Instant};

use objc2_app_kit::NSEvent;
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
    CGEventType, CGMouseButton,
};

use super::keys;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Press,
    Release,
    Click,
}

#[derive(Clone, Copy, Debug)]
pub enum Coordinate {
    Abs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Control,
    Shift,
    Alt,
    Meta,
    Return,
    Tab,
    Escape,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    UpArrow,
    DownArrow,
    LeftArrow,
    RightArrow,
    CapsLock,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Unicode(char),
}

impl Key {
    fn code(self) -> Option<u16> {
        let name = match self {
            Key::Control => return Some(0x3B),
            Key::Shift => return Some(0x38),
            Key::Alt => return Some(0x3A),
            Key::Meta => return Some(0x37),
            Key::CapsLock => return Some(0x39),
            Key::Unicode(' ') => "Space",
            Key::Unicode(c) => {
                let c = c.to_ascii_uppercase();
                return keys::code_from_name(c.encode_utf8(&mut [0; 4]));
            }
            Key::Return => "Enter",
            Key::Tab => "Tab",
            Key::Escape => "Esc",
            Key::Backspace => "Backspace",
            Key::Delete => "Delete",
            Key::Insert => "Insert",
            Key::Home => "Home",
            Key::End => "End",
            Key::PageUp => "PageUp",
            Key::PageDown => "PageDown",
            Key::UpArrow => "Up",
            Key::DownArrow => "Down",
            Key::LeftArrow => "Left",
            Key::RightArrow => "Right",
            Key::F1 => "F1",
            Key::F2 => "F2",
            Key::F3 => "F3",
            Key::F4 => "F4",
            Key::F5 => "F5",
            Key::F6 => "F6",
            Key::F7 => "F7",
            Key::F8 => "F8",
            Key::F9 => "F9",
            Key::F10 => "F10",
            Key::F11 => "F11",
            Key::F12 => "F12",
        };
        keys::code_from_name(name)
    }

    fn flag(self) -> Option<CGEventFlags> {
        Some(match self {
            Key::Control => CGEventFlags::MaskControl,
            Key::Shift => CGEventFlags::MaskShift,
            Key::Alt => CGEventFlags::MaskAlternate,
            Key::Meta => CGEventFlags::MaskCommand,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub struct InputError(&'static str);

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Default)]
pub struct Settings {}

struct LastClick {
    button: Button,
    at: Instant,
    pos: CGPoint,
    count: i64,
}

pub struct Enigo {
    source: Option<CFRetained<CGEventSource>>,
    flags: CGEventFlags,
    // Target of a move whose event may not have been applied yet. The click
    // that follows must use it: the reported cursor position can still be old.
    pending_move: Option<CGPoint>,
    last_click: Option<LastClick>,
    double_click: Duration,
}

/// Cursor position in global top-left-origin points, the space CGEvent uses.
pub fn cursor() -> Option<CGPoint> {
    let event = CGEvent::new(None)?;
    Some(CGEvent::location(Some(&event)))
}

impl Enigo {
    pub fn new(_: &Settings) -> Result<Self, InputError> {
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
        // Never freeze the physical mouse/keyboard after a synthetic event.
        CGEventSource::set_local_events_suppression_interval(source.as_deref(), 0.0);
        Ok(Self {
            source,
            flags: CGEventFlags::empty(),
            pending_move: None,
            last_click: None,
            double_click: Duration::from_secs_f64(NSEvent::doubleClickInterval()),
        })
    }

    pub fn move_mouse(&mut self, x: i32, y: i32, _: Coordinate) -> Result<(), InputError> {
        let pos = CGPoint::new(x as f64, y as f64);
        self.post_mouse(CGEventType::MouseMoved, CGMouseButton::Left, pos, 0)?;
        self.pending_move = Some(pos);
        Ok(())
    }

    pub fn button(&mut self, button: Button, _: Direction) -> Result<(), InputError> {
        let pos = match self.pending_move.take() {
            Some(p) => p,
            None => cursor().ok_or(InputError("cursor position unavailable"))?,
        };
        let (cg, down, up) = match button {
            Button::Left => (
                CGMouseButton::Left,
                CGEventType::LeftMouseDown,
                CGEventType::LeftMouseUp,
            ),
            Button::Right => (
                CGMouseButton::Right,
                CGEventType::RightMouseDown,
                CGEventType::RightMouseUp,
            ),
            Button::Middle => (
                CGMouseButton::Center,
                CGEventType::OtherMouseDown,
                CGEventType::OtherMouseUp,
            ),
        };
        // Count consecutive clicks like the system does for a real mouse: same
        // button, within the double-click interval, at about the same spot.
        let now = Instant::now();
        let count = match &self.last_click {
            Some(l)
                if l.button == button
                    && now - l.at < self.double_click
                    && (l.pos.x - pos.x).abs() < 5.0
                    && (l.pos.y - pos.y).abs() < 5.0 =>
            {
                l.count + 1
            }
            _ => 1,
        };
        self.last_click = Some(LastClick {
            button,
            at: now,
            pos,
            count,
        });
        self.post_mouse(down, cg, pos, count)?;
        self.post_mouse(up, cg, pos, count)
    }

    pub fn key(&mut self, key: Key, direction: Direction) -> Result<(), InputError> {
        let code = key.code().ok_or(InputError("key not available on macOS"))?;
        if direction != Direction::Release {
            self.post_key(key, code, true)?;
        }
        if direction != Direction::Press {
            self.post_key(key, code, false)?;
        }
        Ok(())
    }

    fn post_key(&mut self, key: Key, code: u16, down: bool) -> Result<(), InputError> {
        if let Some(flag) = key.flag() {
            self.flags.set(flag, down);
        }
        let event = CGEvent::new_keyboard_event(self.source.as_deref(), code, down)
            .ok_or(InputError("cannot create keyboard event"))?;
        CGEvent::set_flags(Some(&event), self.flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        Ok(())
    }

    fn post_mouse(
        &self,
        kind: CGEventType,
        button: CGMouseButton,
        pos: CGPoint,
        count: i64,
    ) -> Result<(), InputError> {
        let event = CGEvent::new_mouse_event(self.source.as_deref(), kind, pos, button)
            .ok_or(InputError("cannot create mouse event"))?;
        if count > 0 {
            CGEvent::set_integer_value_field(
                Some(&event),
                CGEventField::MouseEventClickState,
                count,
            );
        }
        CGEvent::set_flags(Some(&event), self.flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        Ok(())
    }
}
