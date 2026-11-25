---
description: "TDD-ordered task breakdown for Spec 002 — Omnibox Inline-Autocomplete Handling"
---

# Tasks: LangFlip — Omnibox Inline-Autocomplete Handling

**Input**: [spec.md](spec.md), [plan.md](plan.md)

## Phase 1: Early Windows 11 UIA Spike (Verify Edge Omnibox Detection)

- [ ] T001 Create minimal Windows 11 UIA spike `crates/langflip-platform-win/examples/uia_selection_poc.rs` that queries `IUIAutomation::GetFocusedElement` → `IUIAutomationTextPattern::GetSelection()` and prints `has_selection=true/false` + query latency in ms; cross-compile and test live on Windows 11 Edge address bar (with and without inline autocomplete) and Notepad.

## Phase 2: Core Seam & ConversionEngine (Strict TDD on Linux)

- [ ] T002 Add `fn has_active_selection(&self) -> bool { false }` to `FocusObserver` in `crates/langflip-core/src/ports.rs`
- [ ] T003 Write failing integration tests in `crates/langflip-core/tests/engine_tests.rs` verifying:
  - When `has_active_selection() == true` on a non-empty burst, `erase_count == N + 1` (1 dismissal Backspace + `N` character Backspaces) while replaying strictly `N` keys and reporting `erased: N + 1, replayed: N`
  - When `has_active_selection() == false`, `erase_count == N`
  - When buffer is empty and `has_active_selection() == true`, 0 Backspaces are emitted (layout toggle only)
  (verify failure)
- [ ] T004 Update `ConversionEngine::trigger_conversion` in `crates/langflip-core/src/engine.rs` until T003 tests pass; run `./scripts/cargo.sh test -p langflip-core --locked --offline`

## Phase 3: Windows 11 UIA Implementation & Verification

- [ ] T005 Implement `WinFocusObserver::has_active_selection` in `crates/langflip-platform-win/src/focus.rs` using thread-local `IUIAutomation` + `IUIAutomationTextPattern::GetSelection()` (returning `false` on any error; never reading/logging text)
- [ ] T006 Run full workspace tests, Linux + Windows `clippy -- -D warnings`, cross-compile `langflip.exe`, copy to `C:\Users\omril\Downloads\langflip.exe`, and verify Edge address bar round-trips cleanly with zero stray characters (and zero regression in Notepad & Windows Terminal)
