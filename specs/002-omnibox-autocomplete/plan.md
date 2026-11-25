# Implementation Plan: LangFlip — Omnibox Inline-Autocomplete Handling

**Branch**: `002-omnibox-autocomplete` | **Date**: 2026-11-26 | **Spec**: [spec.md](spec.md)

## Summary

Eliminate the 1-character drift in Edge/Chrome's address bar (Omnibox) when inline autocomplete highlights a predicted suffix after the cursor. Add a default-`false` `FocusObserver::has_active_selection(&self) -> bool` method in `langflip-core`, use it inside `ConversionEngine::trigger_conversion()` to add 1 dismissal Backspace (`erase_count = count + 1`) while replaying the exact `count` buffered keystrokes, and implement it on Windows 11 via UI Automation (`IUIAutomation::GetFocusedElement` → `IUIAutomationTextPattern::GetSelection`) on the worker thread (never in the hook callback).

## Technical Context

- **Language/Version**: Rust `1.100.0` (isolated `.toolchain/`, `--locked`)
- **New Win32 Features**: `Win32_UI_Accessibility` (already in `Cargo.toml`) + `Win32_System_Com` (already in `Cargo.toml`) — zero new external crates!
- **Testing**: Strict TDD in `langflip-core` on Linux; early Windows 11 UIA spike (`uia_selection_poc.exe`) on Edge's address bar before wiring into `WinFocusObserver`.

## Constitution Check

- **I. Cross-Platform Core**: PASS — `has_active_selection(&self) -> bool` lives on `FocusObserver` in `langflip-core::ports`; UIA lives strictly in `langflip-platform-win`.
- **II. Incremental Complexity**: PASS — Single boolean method on existing seam; zero impact on non-Omnibox apps.
- **III. Isolated Tooling**: PASS — Zero new crates, zero `Cargo.lock` churn.
- **IV. Strict TDD**: PASS — Failing unit/integration tests in `langflip-core` written before production code.
- **V. Privacy & Responsiveness**: PASS — Runs on worker thread (never in hook callback); checks only non-empty range (`CompareEndpoints(Start, End) != 0`), never reads or logs selected text.
- **VI. DRY**: PASS — Reuses `ConversionEngine` and `plan_injection` without duplicating erasure logic.

## Design Notes

1. **Why `has_active_selection()` is safe on a non-empty buffer**:
   - In MVP, user-initiated text selections (`MouseClick` or `Shift+Arrow`) already reset the buffer. Therefore, if `buffer` is non-empty when `Pause` is pressed AND UIA reports a non-empty selection range, that selection was injected automatically by the control (Omnibox/ComboBox autocomplete).
2. **UIA check on Windows 11 (`WinFocusObserver::has_active_selection`)**:
   - Lazily initializes a thread-local `IUIAutomation` COM STA instance on the worker thread (avoids re-initializing COM on every keypress).
   - Calls `uia.GetFocusedElement()` → `GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)` → `GetSelection()`.
   - Iterates returned `IUIAutomationTextRangeArray` and checks `CompareEndpoints(TextPatternRangeEndpoint_Start, &range, TextPatternRangeEndpoint_End) != 0`.
   - On any COM/UIA error or missing `TextPattern`, returns `false` (safe fallback).
3. **Timing**: Call `focus.has_active_selection()` right before `plan_injection(...)` (after layout switch & freshness check).
