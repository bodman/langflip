//! Non-blocking Windows 11 `WH_KEYBOARD_LL` + `WH_MOUSE_LL` + `SetWinEventHook`
//! `InputListener` with bounded `SyncSender` and `PostThreadMessageW(WM_QUIT)`
//! idle shutdown wake-up (`T029`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use langflip_core::hotkey::{HotkeyDecision, HotkeyMatcher, HotkeySpec};
use langflip_core::key::{InjectedBy, InputEvent, KeyEvent, Modifiers, PhysicalKey, scan};
use langflip_core::layout::LayoutId;
use langflip_core::ports::{
    ControlKind, FocusContext, FocusId, InputListener, ListenerShutdown,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardLayout, VK_CANCEL, VK_CAPITAL, VK_LCONTROL,
    VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NUMLOCK, VK_PAUSE, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EVENT_SYSTEM_FOREGROUND, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
    LLKHF_INJECTED, MSG, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WINEVENT_OUTOFCONTEXT, WM_APP, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::focus::foreground_gui_target;
use crate::injector::LANGFLIP_INJECTED_MARKER;

const WM_APP_UPDATE_HOTKEY: u32 = WM_APP + 10;

/// Shutdown & live-config handle for the Windows hook thread.
/// Wakes an idle `GetMessageW` loop immediately via `PostThreadMessageW`.
#[derive(Clone)]
pub struct WinShutdownHandle {
    thread_id: u32,
    stopped: Arc<AtomicBool>,
    pending_hotkey: Arc<Mutex<Option<HotkeySpec>>>,
}

impl WinShutdownHandle {
    /// Applies a new trigger hotkey on the hook thread without locking inside `keyboard_proc`.
    pub fn update_hotkey(&self, spec: HotkeySpec) {
        if let Ok(mut guard) = self.pending_hotkey.lock() {
            *guard = Some(spec);
        }
        let _ = unsafe {
            PostThreadMessageW(
                self.thread_id,
                WM_APP_UPDATE_HOTKEY,
                WPARAM(0),
                LPARAM(0),
            )
        };
    }
}

impl ListenerShutdown for WinShutdownHandle {
    fn stop(&self) {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        }
    }
}

pub struct WinInputListener {
    hkl_map: HashMap<usize, LayoutId>,
    max_callback_us: Arc<AtomicU64>,
}

impl WinInputListener {
    pub fn new(hkl_map: HashMap<usize, LayoutId>, max_callback_us: Arc<AtomicU64>) -> Self {
        Self {
            hkl_map,
            max_callback_us,
        }
    }
}

struct HookState {
    matcher: HotkeyMatcher,
    tx: SyncSender<InputEvent>,
    overflow_flag: Arc<AtomicBool>,
    latest_seq: Arc<AtomicU64>,
    hkl_map: HashMap<usize, LayoutId>,
    max_callback_us: Arc<AtomicU64>,
}

thread_local! {
    static HOOK_STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

impl InputListener for WinInputListener {
    type Error = String;
    type Shutdown = WinShutdownHandle;

    fn run<F>(
        self,
        hotkey: HotkeySpec,
        tx: SyncSender<InputEvent>,
        overflow_flag: Arc<AtomicBool>,
        latest_seq: Arc<AtomicU64>,
        on_ready: F,
    ) -> Result<(), Self::Error>
    where
        F: FnOnce(Self::Shutdown),
    {
        let thread_id = unsafe { GetCurrentThreadId() };
        let stopped = Arc::new(AtomicBool::new(false));
        let pending_hotkey = Arc::new(Mutex::new(None));
        let shutdown = WinShutdownHandle {
            thread_id,
            stopped: stopped.clone(),
            pending_hotkey: pending_hotkey.clone(),
        };

        HOOK_STATE.with(|slot| {
            *slot.borrow_mut() = Some(HookState {
                matcher: HotkeyMatcher::new(hotkey),
                tx,
                overflow_flag,
                latest_seq,
                hkl_map: self.hkl_map,
                max_callback_us: self.max_callback_us,
            });
        });

        let kb_hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0) }
            .map_err(|e| format!("SetWindowsHookExW(WH_KEYBOARD_LL) failed: {e}"))?;

        let mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) }
            .map_err(|e| {
                let _ = unsafe { UnhookWindowsHookEx(kb_hook) };
                format!("SetWindowsHookExW(WH_MOUSE_LL) failed: {e}")
            })?;

        let focus_hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };

        on_ready(shutdown);

        let mut msg = MSG::default();
        while !stopped.load(Ordering::SeqCst) {
            let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if ret.0 <= 0 {
                break;
            }
            if msg.message == WM_APP_UPDATE_HOTKEY
                && let Ok(mut guard) = pending_hotkey.lock()
                && let Some(new_spec) = guard.take()
            {
                HOOK_STATE.with(|slot| {
                    if let Some(state) = slot.borrow_mut().as_mut() {
                        state.matcher.set_spec(new_spec);
                    }
                });
            }
        }

        // Clean teardown of all OS hooks.
        if !focus_hook.is_invalid() {
            let _ = unsafe { UnhookWinEvent(focus_hook) };
        }
        let _ = unsafe { UnhookWindowsHookEx(mouse_hook) };
        let _ = unsafe { UnhookWindowsHookEx(kb_hook) };

        HOOK_STATE.with(|slot| {
            *slot.borrow_mut() = None;
        });
        Ok(())
    }
}

fn enqueue(state: &HookState, ev: InputEvent) {
    if let Err(TrySendError::Full(_)) = state.tx.try_send(ev) {
        state.overflow_flag.store(true, Ordering::SeqCst);
        let seq = state.latest_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = state.tx.try_send(InputEvent::QueueOverflow { seq });
    }
}

fn record_callback_us(state: &HookState, start: Instant) {
    let us = start.elapsed().as_micros() as u64;
    state.max_callback_us.fetch_max(us, Ordering::Relaxed);
}

fn current_modifiers() -> Modifiers {
    let down = |vk: u16| (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000) != 0;
    let toggled = |vk: u16| (unsafe { GetKeyState(i32::from(vk)) } as u16 & 0x0001) != 0;
    Modifiers {
        left_shift: down(VK_LSHIFT.0),
        right_shift: down(VK_RSHIFT.0),
        left_ctrl: down(VK_LCONTROL.0),
        right_ctrl: down(VK_RCONTROL.0),
        left_alt: down(VK_LMENU.0),
        right_alt: down(VK_RMENU.0),
        left_meta: down(VK_LWIN.0),
        right_meta: down(VK_RWIN.0),
        caps_lock: toggled(VK_CAPITAL.0),
        num_lock: toggled(VK_NUMLOCK.0),
    }
}

fn normalize_physical_key(kb: &KBDLLHOOKSTRUCT) -> PhysicalKey {
    // Disambiguate Pause vs NumLock (both use scan code 0x45 on AT Set 1).
    let vk = kb.vkCode as u16;
    if vk == VK_PAUSE.0 || vk == VK_CANCEL.0 {
        return scan::PAUSE;
    }
    if vk == VK_NUMLOCK.0 {
        return scan::NUM_LOCK;
    }
    let extended = (kb.flags.0 & LLKHF_EXTENDED.0) != 0;
    PhysicalKey::new((kb.scanCode & 0xFF) as u16, extended)
}

fn current_layout_and_focus(hkl_map: &HashMap<usize, LayoutId>) -> (LayoutId, FocusId) {
    let Some((hwnd, tid)) = foreground_gui_target() else {
        return (LayoutId::new(""), FocusId(0));
    };
    let hkl = unsafe { GetKeyboardLayout(tid) };
    let raw = hkl.0 as usize;
    let layout = hkl_map.get(&raw).cloned().unwrap_or_else(|| {
        let lang_id = (raw & 0xFFFF) as u16;
        LayoutId::new(format!("{lang_id:08X}"))
    });
    (layout, FocusId(hwnd.0 as usize as u64))
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }
    let start = Instant::now();
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };

    // Fast path: ignore our own self-injected events immediately.
    if kb.dwExtraInfo == LANGFLIP_INJECTED_MARKER {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }

    let msg = wparam.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    if !is_down && !is_up {
        return unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) };
    }

    let injected = if (kb.flags.0 & LLKHF_INJECTED.0) != 0 {
        InjectedBy::External
    } else {
        InjectedBy::Physical
    };
    let key = normalize_physical_key(kb);
    let modifiers = current_modifiers();

    let mut swallow = false;
    HOOK_STATE.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            let (active_layout, focus_id) = current_layout_and_focus(&state.hkl_map);
            if is_down {
                if injected == InjectedBy::Physical {
                    match state.matcher.on_key_down(key, modifiers) {
                        HotkeyDecision::SwallowRepeat => {
                            swallow = true;
                            record_callback_us(state, start);
                            return;
                        }
                        HotkeyDecision::Trigger => {
                            swallow = true;
                            // Trigger does NOT advance latest_seq!
                            let seq = state.latest_seq.load(Ordering::SeqCst);
                            enqueue(
                                state,
                                InputEvent::KeyDown(KeyEvent {
                                    key,
                                    modifiers,
                                    injected,
                                    active_layout,
                                    focus_id,
                                    seq,
                                }),
                            );
                            record_callback_us(state, start);
                            return;
                        }
                        HotkeyDecision::PassThrough => {}
                    }
                }
                // Mutating non-hotkey KeyDown advances latest_seq.
                let seq = state.latest_seq.fetch_add(1, Ordering::SeqCst) + 1;
                enqueue(
                    state,
                    InputEvent::KeyDown(KeyEvent {
                        key,
                        modifiers,
                        injected,
                        active_layout,
                        focus_id,
                        seq,
                    }),
                );
            } else {
                // KeyUp NEVER advances latest_seq (so releasing Pause/Ctrl+Shift never cancels).
                state.matcher.on_key_up(key);
                let seq = state.latest_seq.load(Ordering::SeqCst);
                enqueue(
                    state,
                    InputEvent::KeyUp(KeyEvent {
                        key,
                        modifiers,
                        injected,
                        active_layout,
                        focus_id,
                        seq,
                    }),
                );
            }
            record_callback_us(state, start);
        }
    });

    if swallow {
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) }
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN {
            HOOK_STATE.with(|slot| {
                if let Some(state) = slot.borrow().as_ref() {
                    let (_, focus_id) = current_layout_and_focus(&state.hkl_map);
                    let seq = state.latest_seq.fetch_add(1, Ordering::SeqCst) + 1;
                    enqueue(state, InputEvent::MouseClick { focus_id, seq });
                }
            });
        }
    }
    unsafe { CallNextHookEx(Some(HHOOK::default()), code, wparam, lparam) }
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND {
        HOOK_STATE.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                let seq = state.latest_seq.fetch_add(1, Ordering::SeqCst) + 1;
                enqueue(
                    state,
                    InputEvent::FocusChanged {
                        focus: FocusContext {
                            focus_id: FocusId(hwnd.0 as usize as u64),
                            element_id: None,
                            control_kind: ControlKind::Unknown,
                        },
                        seq,
                    },
                );
            }
        });
    }
}
