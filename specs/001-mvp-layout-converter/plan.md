# Implementation Plan: LangFlip MVP — In-Place Keyboard Layout Converter

**Branch**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revision 3 after Multi-Agent Design Review) | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/001-mvp-layout-converter/spec.md`

## Summary

Build **LangFlip**, a lightweight, cross-platform-architected Rust utility initially targeting Windows 11 that converts mistyped straight-line 1:1 text between configurable keyboard layouts (defaulting to `en-US` ↔ `he-IL`) in-place across browsers, native apps, and command-line terminals on a configurable physical-key hotkey (default: `Pause`).

All domain logic — key classification, physical-key hotkey matching & auto-repeat suppression, layout-pair resolution, bounded RAM-only buffering, injection sequence planning (including mask key and exact left/right modifier release), and safe conversion orchestration (`Check Layout -> Check Freshness -> Switch & Confirm -> Re-check Freshness -> Plan & Execute Injection -> Retain Buffer on Full Success`) — lives in `langflip-core` with zero OS dependencies and 100% strict TDD coverage on Linux. The Windows 11 adapter (`langflip-platform-win`) is a thin Win32 translator validated early via a dedicated Windows 11 PoC spike right after Phase 1.

## Technical Context

**Language/Version**: Rust exact pinned release `1.100.0` (`rust-toolchain.toml` with `clippy` + `rustfmt`, installed strictly in `<repo>/.toolchain/` via checksum-verified `rustup-init --no-modify-path`)

**Primary Dependencies**:
- `langflip-core`: `serde`, `toml` (zero OS-specific crates)
- `langflip-platform-win`: `[target.'cfg(windows)'.dependencies] windows` (official Microsoft Win32 bindings: `Foundation`, `WindowsAndMessaging`, `KeyboardAndMouse`, `Threading`, `Registry`, `Globalization`, `Accessibility`)
- `langflip-app`: `langflip-core`, `langflip-platform-win` (Windows target), `ctrlc`

**Storage**: Optional local TOML config (`<exe_dir>/langflip.toml` → `<os_config_dir>/langflip/langflip.toml` → defaults; fail-fast on invalid config); zero persistence or logging of keystrokes (volatile RAM only, redacted `Debug`).

**Testing**: Strict Red-Green-Refactor TDD via `cargo test` natively on Linux for all core logic (`classify`, `HotkeyMatcher`, `resolve_layouts`, `plan_injection`, `KeystrokeBuffer`, `ConversionEngine`, `AppConfig` + `langflip.example.toml` parse test); Linux + Windows cross-target `clippy -- -D warnings`; early Windows 11 PoC spike + full Windows 11 verification checklist.

**Target Platform**: Windows 11 (`x86_64-pc-windows-gnu`, cross-compiled from Linux with `--locked`); core crate compiles and tests natively on Linux.

**Project Type**: 3-crate Cargo workspace (OS-independent core library + thin Windows adapter library + thin app binary).

**Performance Goals**: Hook callback latency < 1 ms (non-blocking `try_send` into bounded `SyncSender(256)`); 50-char conversion + confirmed layout switch < 150 ms (`replay_delay_ms = 0` default); idle `Ctrl+C` shutdown < 500 ms via `WM_QUIT` wake-up.

**Constraints**: Zero global toolchain/shell-profile mutation (`~/.rustup`, `~/.cargo`, `~/.profile`, `~/.bashrc` untouched); hard memory bounds on both raw input queue (`256`) and keystroke buffer (`1..=512`, default `128`); zero keystroke logging in any build; no erasure before confirmed layout switch.

**Scale/Scope**: Single-user desktop utility, ~1,400–2,000 LOC across 3 crates for MVP.

## Constitution Check

*GATE: Checked against [constitution.md](../../.specify/memory/constitution.md) v1.1.1.*

- **I. Cross-Platform Core, Thin OS Adapters**: PASS — All pure logic (`classify_key`, `HotkeyMatcher`, `resolve_layouts`, `plan_injection`, `KeystrokeBuffer`, `ConversionEngine`, `AppConfig`) lives in `langflip-core` with zero OS dependencies; Win32 calls live strictly in `langflip-platform-win`.
- **II. Incremental Complexity**: PASS — MVP implements `NaiveBurstPolicy` and foreground `SetWinEventHook` `FocusObserver`, preserving clean seams for Phase 2 (UIA multi-line/cursor-aware buffer + auto layout discovery), Phase 3 (UIA text reading + multi-char keys + selection fallback), and Phase 4 (tray UI + macOS/Linux).
- **III. Isolated & Reproducible Tooling**: PASS — `toolchain/deps.conf` (with pinned `rustup-init` SHA-256), exact `rust-toolchain.toml` (`1.100.0` + `clippy`/`rustfmt`), and committed `Cargo.lock` (`--locked`) enforce isolation inside `.toolchain/` with `--no-modify-path`.
- **IV. Strict Test-Driven Development (TDD — NON-NEGOTIABLE)**: PASS — Every pure core module (including key classification, hotkey/auto-repeat matching, layout resolution, and replay sequence planning) begins with failing Linux unit/integration tests before production code is written.
- **V. Privacy, Responsiveness & Non-Intrusiveness**: PASS — Hard-bounded `SyncSender(256)` + `KeystrokeBuffer(1..=512)`, redacted `Debug`, zero keystroke logging in debug or release builds, non-blocking `try_send` hooks, `WM_QUIT` idle shutdown wake-up.
- **VI. Reuse Over Re-Implementation (DRY)**: PASS — Single `HotkeyMatcher` (used by both hook and tests, zero duplication), single `resolve_layouts`, single `plan_injection`, single `ConversionEngine`.

## Project Structure

### Documentation (this feature)

```text
specs/001-mvp-layout-converter/
├── plan.md              # This file
├── research.md          # Phase 0 technical decisions
├── data-model.md        # Core entities, config schema, state transitions
├── quickstart.md        # Isolated toolchain setup & Windows 11 verification checklist
├── contracts/
│   └── platform-traits.md
└── tasks.md             # TDD-ordered task breakdown
```

### Source Code (repository root)

```text
langflip/
├── .cargo/
│   └── config.toml                 # x86_64-pc-windows-gnu linker config
├── .toolchain/                     # Gitignored isolated RUSTUP_HOME & CARGO_HOME
├── Cargo.toml                      # Workspace manifest
├── Cargo.lock                      # Committed lockfile (regenerated after crate deps added)
├── rust-toolchain.toml             # Exact Rust 1.100.0 + clippy/rustfmt + targets
├── langflip.example.toml           # Documented example config (tested by unit test)
├── toolchain/
│   └── deps.conf                   # Declarative host deps + pinned rustup-init checksum
├── scripts/
│   ├── setup-env.sh                # Audits/installs deps + isolated Rust (--no-modify-path)
│   └── cargo.sh                    # Wrapper enforcing isolated env vars
└── crates/
    ├── langflip-core/              # Platform-independent domain & traits (100% Linux TDD)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── ports.rs            # FocusId, ControlKind, FocusContext, InjectionStep, OS traits
    │       ├── key.rs              # PhysicalKey, KeyName, left/right Modifiers, classify_key, InputEvent
    │       ├── hotkey.rs           # HotkeySpec, HotkeyMatcher (single source of truth + auto-repeat)
    │       ├── layout.rs           # Opaque LayoutId, InstalledLayout, LayoutPair, resolve_layouts
    │       ├── config.rs           # AppConfig, lookup order, strict fail-fast TOML validation
    │       ├── buffer.rs           # BufferPolicy, NaiveBurstPolicy, redacted KeystrokeBuffer
    │       ├── planner.rs          # Pure plan_injection (mask key, L/R modifier release, erase, replay)
    │       └── engine.rs           # ConversionEngine (safe Switch->Confirm->Freshness->Inject runner)
    │   └── tests/
    │       └── engine_tests.rs     # Focused mocked integration tests
    ├── langflip-platform-win/      # Thin Windows 11 Win32 adapter
    │   ├── Cargo.toml              # [target.'cfg(windows)'.dependencies] windows
    │   ├── examples/
    │   │   └── win11_poc.rs        # Early Windows 11 spike (Phase 1.5)
    │   └── src/
    │       ├── lib.rs
    │       ├── hook.rs             # WH_KEYBOARD_LL + WH_MOUSE_LL + SetWinEventHook + WM_QUIT shutdown
    │       ├── injector.rs         # Thin SendInput translator for InjectionStep
    │       ├── layout.rs           # GetGUIThreadInfo, registry KLID lookup, WM_INPUTLANGCHANGEREQUEST + confirm
    │       └── focus.rs            # Foreground HWND FocusObserver (Phase 2 UIA seam)
    └── langflip-app/               # Thin binary entry point
        ├── Cargo.toml
        └── src/
            └── main.rs             # Config load, layout validation warnings, event loop, --metrics, Ctrl+C
```

**Structure Decision**: A 3-crate Cargo workspace enforces compile-time isolation between pure OS-independent logic (`langflip-core`) and Win32 bindings (`langflip-platform-win`), guaranteeing native Linux TDD, Windows cross-target Clippy, and drop-in future OS backends.

## Complexity Tracking

No constitution violations. All 3 crates are directly justified by Principle I (compile-time OS isolation) and Principle IV (strict native Linux TDD).
