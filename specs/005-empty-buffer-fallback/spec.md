# Feature Specification: Empty-Buffer Current-Line / Single-Line Fallback

**Feature Branch**: `005-empty-buffer-fallback`  
**Created**: 2026-11-26  
**Status**: Implemented  

## Problem Statement

When a user accidentally presses a focus-stealing neighbor key (such as `PrintScreen` launching Windows 11's Snipping Tool right next to `Pause`), or switches back to a window/tab with a mistyped line, the live `KeystrokeBuffer` is reset by `FocusChanged` / `MouseClick`. Pressing the LangFlip hotkey immediately afterward finds an empty buffer and only toggles the active keyboard layout without converting the mistyped text.

## Solution (Current-Line / Single-Line Selection Fallback)

When the trigger hotkey is pressed while the live keystroke buffer is empty:

1. **UIA Safety Gate + `Shift+Home` / `Ctrl+C` Fallback (`FocusObserver::read_line_before_caret`)**:
   - Never runs in terminal windows (`CASCADIA_HOSTING_WINDOW_CLASS`, `ConsoleWindowClass`, `mintty`, `Alacritty`, `WezTerm`).
   - Uses Windows UI Automation (`IUIAutomation::GetFocusedElement`) as a safety gate: never reads password fields (`CurrentIsPassword == true`), read-only controls (`IUIAutomationValuePattern::CurrentIsReadOnly == true`), or non-text widgets (buttons, checkboxes, menus, file lists, tabs, etc.).
   - Saves any existing `CF_UNICODETEXT` clipboard content and checks `GetClipboardSequenceNumber()`.
   - Unless text is already selected, sends synthetic `Shift+Home` (tagged with `LANGFLIP_INJECTED_MARKER` so the hook ignores it) to select the current line back to the logical start, followed by `Ctrl+C`.
   - Polls `GetClipboardSequenceNumber()` up to 60 ms; if unchanged (empty line or non-editable target), returns `None` without touching the clipboard.
   - Immediately restores the user's previous clipboard content.
   - Rejects multi-line selections (`\r`/`\n`), whitespace-only strings, or strings exceeding `config.max_buffer_len` (collapsing any synthetic selection via `Right` arrow).

2. **OS-Native Reverse Keystroke Mapping (`LayoutController::text_to_keystrokes`)**:
   - Maps each UTF-16 BMP character back to a physical `BufferedKeystroke` (`scan_code`, `extended`, `shift`) using `VkKeyScanExW` + `MapVirtualKeyExW(MAPVK_VK_TO_VSC_EX)` against the layout's `HKL`.
   - Aborts (`None`) on any character requiring Ctrl/AltGr, surrogate pairs/emojis, or characters absent from the layout.

3. **Smart Direction Resolution & Round-Trip Retention (`ConversionEngine`)**:
   - If `current_layout` is in the configured pair (`target_layout = opposite`):
     - **Normal case**: Text maps cleanly under `current_layout` → switch & confirm `target_layout`, verify freshness, erase (`1` Backspace if `replaces_selection`, else `chars.count()` Backspaces), replay mapped strokes, and populate the live `KeystrokeBuffer` so a second hotkey press cleanly round-trips.
     - **Already-toggled case**: Text does not map under `current_layout`, but maps cleanly under `target_layout` → keep `current_layout` (no switch needed), verify freshness, erase, replay mapped strokes in `current_layout`, and populate the live `KeystrokeBuffer`.
   - If no fallback text is available, or text is whitespace-only, or cannot be mapped under either layout, or active layout is a third unrelated layout → never erase anything; fall back to existing `toggle_on_empty_buffer` behavior.

4. **Privacy Invariant**:
   - `FallbackText` lives strictly in RAM for the duration of the trigger call and its `Debug` implementation redacts the text content (printing only character count and `replaces_selection`).
