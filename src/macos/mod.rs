//! macOS integration: permissions, key capture, power/timer behavior.

pub mod hotkeys;
pub mod input;
pub mod keys;

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSWindow};
use objc2_core_foundation::{kCFBooleanTrue, CFBoolean, CFDictionary, CFString, CFType};
use objc2_foundation::{NSActivityOptions, NSPoint, NSProcessInfo, NSString};

use crate::app::SharedState;
use keys::Mods;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const CFDictionary) -> bool;
    static kAXTrustedCheckOptionPrompt: &'static CFString;
}

/// Whether macOS lets this process post input events (Accessibility).
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Show the system's Accessibility prompt if access is missing.
pub fn request_access() -> bool {
    unsafe {
        let Some(yes) = kCFBooleanTrue else {
            return is_trusted();
        };
        let yes: &CFBoolean = yes;
        let opts = CFDictionary::<CFString, CFType>::from_slices(
            &[kAXTrustedCheckOptionPrompt],
            &[yes.as_ref()],
        );
        AXIsProcessTrustedWithOptions((&*opts as *const CFDictionary<CFString, CFType>).cast())
    }
}

pub fn open_accessibility_settings() {
    let _ = std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

/// Start the clicker, unless macOS would silently drop the events.
pub fn activate(shared: &SharedState) {
    if is_trusted() {
        shared.set_active(true);
    } else {
        request_access();
    }
}

/// The app clicks while in the background: opt out of App Nap and timer
/// coalescing for the whole run.
pub fn keep_awake_in_background() {
    let reason = NSString::from_str("Auto clicking");
    let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep
            | NSActivityOptions::LatencyCritical,
        &reason,
    );
    std::mem::forget(token);
}

fn capturing(shared: &SharedState) -> bool {
    [
        &shared.hotkey_capture_active,
        &shared.sequence_hotkey_capture_active,
        &shared.key_capture_active,
    ]
    .iter()
    .any(|f| f.load(Ordering::Relaxed))
}

/// Capture key presses for the bind chips. Reads keycodes rather than typed
/// text, so it works under any keyboard layout and with Option held. A local
/// monitor only sees this app's own events and needs no permission.
pub fn install_key_capture(shared: Arc<SharedState>) {
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let ev = unsafe { event.as_ref() };
        if !capturing(&shared) {
            return event.as_ptr();
        }
        let f = ev.modifierFlags();
        let mods = Mods {
            ctrl: f.contains(NSEventModifierFlags::Control),
            shift: f.contains(NSEventModifierFlags::Shift),
            alt: f.contains(NSEventModifierFlags::Option),
            meta: f.contains(NSEventModifierFlags::Command),
        };
        if !ev.isARepeat() {
            if let Some(combo) = keys::format_combo(ev.keyCode(), mods) {
                if let Ok(mut s) = shared.settings.write() {
                    let take = |flag: &AtomicBool| flag.swap(false, Ordering::Relaxed);
                    if take(&shared.hotkey_capture_active) {
                        s.hotkey = combo.clone();
                    }
                    if take(&shared.sequence_hotkey_capture_active) {
                        s.sequence_hotkey = combo.clone();
                    }
                    if take(&shared.key_capture_active) {
                        s.click_key = combo;
                    }
                }
            }
        }
        std::ptr::null_mut()
    });
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
    };
    std::mem::forget(monitor);
}

struct Drag {
    window: Retained<NSWindow>,
    // Where in the window the press landed, in window coordinates.
    grab: NSPoint,
    moving: bool,
}

thread_local! {
    static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
}

/// Window dragging by hand. `performWindowDragWithEvent` (winit's
/// `drag_window`) must run while AppKit dispatches the press, but winit
/// delivers it later and the system then refuses to start the move.
pub fn install_drag_support(mtm: MainThreadMarker) {
    let mask =
        NSEventMask::LeftMouseDown | NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp;
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let ev = unsafe { event.as_ref() };
        DRAG.with_borrow_mut(|drag| match ev.r#type() {
            NSEventType::LeftMouseDown => {
                *drag = ev.window(mtm).map(|window| Drag {
                    window,
                    grab: ev.locationInWindow(),
                    moving: false,
                });
            }
            NSEventType::LeftMouseUp => *drag = None,
            _ => {}
        });
        // Moving the window can re-enter event handling: do it unborrowed.
        let target = DRAG.with_borrow(|d| {
            d.as_ref()
                .filter(|d| d.moving)
                .map(|d| (d.window.clone(), d.grab))
        });
        if let Some((window, grab)) = target {
            let mouse = NSEvent::mouseLocation();
            window.setFrameOrigin(NSPoint::new(mouse.x - grab.x, mouse.y - grab.y));
        }
        event.as_ptr()
    });
    std::mem::forget(unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block)
    });
}

/// Let the window follow the mouse until the held button is released.
pub fn drag_window() {
    DRAG.with_borrow_mut(|d| {
        if let Some(d) = d {
            d.moving = true;
        }
    });
}
