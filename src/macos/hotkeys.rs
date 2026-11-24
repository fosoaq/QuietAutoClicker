//! Global hotkeys through Carbon `RegisterEventHotKey`, which needs no
//! Input Monitoring permission and reports both press and release.
//! Everything here runs on the main thread.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::{atomic::Ordering, Arc};

use super::keys::{self, Mods};
use crate::app::{ActivationMode, SequencePoint, SharedState};

#[repr(C)]
#[derive(Clone, Copy)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

type Handler = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: Handler,
        num_types: usize,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        mods: u32,
        id: HotKeyId,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
    fn GetEventKind(event: *mut c_void) -> u32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        ty: u32,
        out_ty: *mut u32,
        size: usize,
        out_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
}

const SIGNATURE: u32 = u32::from_be_bytes(*b"QACk");
const CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const TYPE_HOTKEY_ID: u32 = u32::from_be_bytes(*b"hkid");
const PRESSED: u32 = 5;
const RELEASED: u32 = 6;

const ID_TOGGLE: u32 = 1;
const ID_CAPTURE: u32 = 2;
const MAX_POINTS: usize = 10;

struct Slot {
    id: u32,
    combo: String,
    handle: *mut c_void,
}

impl Slot {
    const fn new(id: u32) -> Self {
        Self {
            id,
            combo: String::new(),
            handle: std::ptr::null_mut(),
        }
    }

    fn release(&mut self) {
        if !self.handle.is_null() {
            unsafe { UnregisterEventHotKey(self.handle) };
            self.handle = std::ptr::null_mut();
        }
        self.combo.clear();
    }

    fn bind(&mut self, combo: &str) {
        if self.combo == combo {
            return;
        }
        self.release();
        self.combo = combo.to_string();
        let Some((code, mods)) = keys::parse_combo(combo) else {
            return;
        };
        let id = HotKeyId {
            signature: SIGNATURE,
            id: self.id,
        };
        let status = unsafe {
            RegisterEventHotKey(
                code as u32,
                carbon_mods(mods),
                id,
                GetApplicationEventTarget(),
                0,
                &mut self.handle,
            )
        };
        crate::sequence::write_diag(&format!("hotkeys: bind {combo} -> status {status}"));
    }
}

thread_local! {
    static SLOTS: RefCell<[Slot; 2]> =
        const { RefCell::new([Slot::new(ID_TOGGLE), Slot::new(ID_CAPTURE)]) };
}

fn carbon_mods(m: Mods) -> u32 {
    (m.meta as u32) << 8 | (m.shift as u32) << 9 | (m.alt as u32) << 11 | (m.ctrl as u32) << 12
}

extern "C" fn on_event(_: *mut c_void, event: *mut c_void, user: *mut c_void) -> i32 {
    const NOT_HANDLED: i32 = -9874;
    let mut id = HotKeyId {
        signature: 0,
        id: 0,
    };
    let status = unsafe {
        GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_HOTKEY_ID,
            std::ptr::null_mut(),
            std::mem::size_of::<HotKeyId>(),
            std::ptr::null_mut(),
            (&mut id as *mut HotKeyId).cast(),
        )
    };
    if status != 0 || id.signature != SIGNATURE {
        return NOT_HANDLED;
    }
    // The state is leaked in install(), so it outlives every callback.
    let shared = unsafe { &*(user as *const SharedState) };
    match (id.id, unsafe { GetEventKind(event) }) {
        (ID_TOGGLE, PRESSED) => toggle_pressed(shared),
        (ID_TOGGLE, RELEASED) => toggle_released(shared),
        (ID_CAPTURE, PRESSED) => add_point(shared),
        _ => {}
    }
    0
}

fn mode(shared: &SharedState) -> Option<ActivationMode> {
    shared.settings.read().ok().map(|s| s.activation_mode)
}

fn toggle_pressed(shared: &SharedState) {
    match mode(shared) {
        Some(ActivationMode::Toggle) if shared.is_active() => shared.set_active(false),
        Some(_) => super::activate(shared),
        None => {}
    }
    crate::sequence::write_diag(&format!(
        "hotkeys: toggle -> active = {}",
        shared.is_active()
    ));
}

fn toggle_released(shared: &SharedState) {
    if matches!(mode(shared), Some(ActivationMode::Hold)) {
        shared.set_active(false);
    }
}

fn add_point(shared: &SharedState) {
    if !shared.advanced_mode.load(Ordering::Relaxed)
        || !shared.sequence_enabled.load(Ordering::Relaxed)
    {
        return;
    }
    let Some(pos) = super::input::cursor() else {
        return;
    };
    let (mut points, r#loop, smooth, smooth_ms) = shared.get_sequence_data();
    if points.len() < MAX_POINTS {
        points.push(SequencePoint {
            x: pos.x.round() as i32,
            y: pos.y.round() as i32,
            clicks: 1,
        });
        shared.set_sequence_data(points, r#loop, smooth, smooth_ms);
    }
}

pub fn install(shared: Arc<SharedState>) {
    let types = [
        EventTypeSpec {
            class: CLASS_KEYBOARD,
            kind: PRESSED,
        },
        EventTypeSpec {
            class: CLASS_KEYBOARD,
            kind: RELEASED,
        },
    ];
    let status = unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            on_event,
            types.len(),
            types.as_ptr(),
            Arc::into_raw(shared) as *mut c_void,
            std::ptr::null_mut(),
        )
    };
    crate::sequence::write_diag(&format!("hotkeys: handler status {status}"));
}

/// Reconcile the registered hotkeys with the settings. While `suspended` (a key
/// is being captured) nothing is registered, so a bound key can be captured
/// again. Returns whether each hotkey is currently usable.
pub fn sync(toggle: &str, capture: &str, suspended: bool) -> (bool, bool) {
    SLOTS.with_borrow_mut(|[t, c]| {
        if suspended {
            t.release();
            c.release();
            return (true, true);
        }
        // Free both before rebinding, so swapped keys do not collide.
        if t.combo != toggle {
            t.release();
        }
        if c.combo != capture {
            c.release();
        }
        t.bind(toggle);
        c.bind(capture);
        (!t.handle.is_null(), !c.handle.is_null())
    })
}
