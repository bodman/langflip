//! Thin Windows 11 `SendInput` translator for `langflip_core::ports::InjectionStep` (`T026`).

use std::thread;
use std::time::Duration;

use langflip_core::key::{PhysicalKey, scan};
use langflip_core::ports::{InjectError, InjectionStep, InputInjector};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY,
};

use crate::focus::is_win11_notepad_foreground;

/// Distinct `dwExtraInfo` marker tagging every synthetic event emitted by LangFlip.
pub const LANGFLIP_INJECTED_MARKER: usize = 0x4C46_4C50;

const VK_MASK_UNASSIGNED: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);
const WINUI_NOTEPAD_HOLD_MS: u64 = 4;
const WINUI_NOTEPAD_GAP_MS: u64 = 12;

#[derive(Debug, Default)]
pub struct WinInputInjector;

impl InputInjector for WinInputInjector {
    fn execute_steps(&mut self, steps: &[InjectionStep]) -> Result<(), InjectError> {
        if is_win11_notepad_foreground() {
            return execute_steps_paced(steps);
        }

        let mut batch: Vec<INPUT> = Vec::with_capacity(steps.len() * 2);

        for step in steps {
            match *step {
                InjectionStep::MaskKey => {
                    batch.push(mask_key_input());
                }
                InjectionStep::ReleaseModifier(key) => {
                    batch.push(scan_input(key, true));
                }
                InjectionStep::KeyTap { key, shift } => {
                    if shift {
                        batch.push(scan_input(scan::LEFT_SHIFT, false));
                    }
                    batch.push(scan_input(key, false));
                    batch.push(scan_input(key, true));
                    if shift {
                        batch.push(scan_input(scan::LEFT_SHIFT, true));
                    }
                }
                InjectionStep::DelayMs(ms) => {
                    flush_batch(&mut batch)?;
                    if ms > 0 {
                        thread::sleep(Duration::from_millis(ms));
                    }
                }
            }
        }

        flush_batch(&mut batch)
    }
}

/// Paced per-event injection for Windows 11 WinUI 3 Notepad (`RichEditD2DPT`),
/// separating `KEYDOWN` and `KEYUP` so its async input queue never drops or
/// transposes characters, without touching the user's clipboard.
fn execute_steps_paced(steps: &[InjectionStep]) -> Result<(), InjectError> {
    let mut one = Vec::with_capacity(1);
    for step in steps {
        match *step {
            InjectionStep::MaskKey => {
                one.push(mask_key_input());
                flush_batch(&mut one)?;
            }
            InjectionStep::ReleaseModifier(key) => {
                one.push(scan_input(key, true));
                flush_batch(&mut one)?;
            }
            InjectionStep::KeyTap { key, shift } => {
                if shift {
                    one.push(scan_input(scan::LEFT_SHIFT, false));
                    flush_batch(&mut one)?;
                    thread::sleep(Duration::from_millis(WINUI_NOTEPAD_HOLD_MS));
                }
                one.push(scan_input(key, false));
                flush_batch(&mut one)?;
                thread::sleep(Duration::from_millis(WINUI_NOTEPAD_HOLD_MS));
                one.push(scan_input(key, true));
                flush_batch(&mut one)?;
                if shift {
                    thread::sleep(Duration::from_millis(WINUI_NOTEPAD_HOLD_MS));
                    one.push(scan_input(scan::LEFT_SHIFT, true));
                    flush_batch(&mut one)?;
                }
                thread::sleep(Duration::from_millis(WINUI_NOTEPAD_GAP_MS));
            }
            InjectionStep::DelayMs(_) => {
                // Already paced by WINUI_NOTEPAD_GAP_MS.
            }
        }
    }
    Ok(())
}

fn mask_key_input() -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_MASK_UNASSIGNED,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: LANGFLIP_INJECTED_MARKER,
            },
        },
    }
}

fn scan_input(key: PhysicalKey, key_up: bool) -> INPUT {
    let mut flags: KEYBD_EVENT_FLAGS = KEYEVENTF_SCANCODE;
    if key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: key.scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: LANGFLIP_INJECTED_MARKER,
            },
        },
    }
}

fn flush_batch(batch: &mut Vec<INPUT>) -> Result<(), InjectError> {
    if batch.is_empty() {
        return Ok(());
    }
    let expected = batch.len();
    let sent = unsafe { SendInput(batch, std::mem::size_of::<INPUT>() as i32) } as usize;
    batch.clear();
    if sent != expected {
        return Err(InjectError::PartialInjection { expected, sent });
    }
    Ok(())
}
