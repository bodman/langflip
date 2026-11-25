//! Spec 002 Phase 1 Windows 11 UIA Spike (`T001`).
//!
//! Verifies whether `IUIAutomation::GetFocusedElement` -> `IUIAutomationTextPattern::GetSelection`
//! reliably detects Edge/Chrome Omnibox inline-autocomplete selection (without reading any text).

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("uia_selection_poc is Windows-only.");
}

#[cfg(target_os = "windows")]
fn main() {
    win_impl::run();
}

#[cfg(target_os = "windows")]
mod win_impl {
    use std::thread;
    use std::time::{Duration, Instant};

    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationTextPattern, TextPatternRangeEndpoint_End,
        TextPatternRangeEndpoint_Start, UIA_TextPatternId,
    };

    pub fn run() {
        println!("=== LangFlip Spec 002 UIA Selection Spike ===");
        println!("Focus Edge address bar (type e.g. 'goo' so autocomplete highlights) or Notepad.");
        println!("Polling every 1s for 8 seconds...\n");

        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let uia: IUIAutomation = match unsafe {
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        } {
            Ok(u) => u,
            Err(e) => {
                eprintln!("CoCreateInstance(CUIAutomation) failed: {e}");
                unsafe { CoUninitialize() };
                return;
            }
        };

        for tick in 1..=8 {
            let start = Instant::now();
            let (has_pattern, has_selection, control_name) = query_selection(&uia);
            println!(
                "  [t={tick}s] text_pattern={has_pattern:<5} has_selection={has_selection:<5} elapsed={}ms control=\"{control_name}\"",
                start.elapsed().as_millis()
            );
            thread::sleep(Duration::from_secs(1));
        }

        unsafe {
            CoUninitialize();
        }
        println!("\n=== UIA Spike complete ===");
    }

    fn query_selection(uia: &IUIAutomation) -> (bool, bool, String) {
        let Ok(elem) = (unsafe { uia.GetFocusedElement() }) else {
            return (false, false, String::from("<no focused element>"));
        };
        let name = unsafe { elem.CurrentName() }
            .map(|b| b.to_string())
            .unwrap_or_default();

        let Ok(pattern) =
            (unsafe { elem.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) })
        else {
            return (false, false, name);
        };

        let Ok(ranges) = (unsafe { pattern.GetSelection() }) else {
            return (true, false, name);
        };

        let len = unsafe { ranges.Length() }.unwrap_or(0);
        for i in 0..len {
            if let Ok(range) = unsafe { ranges.GetElement(i) } {
                let cmp = unsafe {
                    range.CompareEndpoints(
                        TextPatternRangeEndpoint_Start,
                        &range,
                        TextPatternRangeEndpoint_End,
                    )
                };
                if matches!(cmp, Ok(v) if v != 0) {
                    return (true, true, name);
                }
            }
        }

        (true, false, name)
    }
}
