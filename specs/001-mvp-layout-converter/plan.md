# Implementation Plan: LangFlip MVP — In-Place Keyboard Layout Converter

**Branch**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revised after Design Review) | **Spec**: [spec.md](file:///home/omri/projects/langflip/specs/001-mvp-layout-converter/spec.md)

**Input**: Feature specification from `/specs/001-mvp-layout-converter/spec.md`

## Summary

Build **LangFlip**, a lightweight, cross-platform-architected Rust utility initially targeting Windows 11 that converts mistyped straight-line text between configurable keyboard layouts (defaulting to US QWERTY `"00000409"` ↔ Israeli Hebrew Standard `"0000040D"`) in-place across browsers, native apps, and command-line terminals on a configurable hotkey (default: `Pause/Break`).

Conversion executes strictly in **Switch → Confirm → Validate Snapshot → Erase → Replay** order so rejected switches, elevated windows, or rapid post-hotkey typing/focus changes can never delete user text. All OS-specific Win32 hooks, `KLID`/`HKL` resolution, and `SendInput` injection stay isolated in `langflip-platform-win` behind narrow traits in `langflip-core`, enabling 100% native Linux TDD and clean Phase 2+ growth (UI Automation focus-aware buffers, automatic layout discovery, macOS/Linux backends). All host and Rust dependencies are declaratively configured (`toolchain/deps.conf`, exact pinned `rust-toolchain.toml`, committed `Cargo.lock`) and bootstrapped strictly inside `<repo>/.toolchain/`.

## Technical Context

**Language/Version**: Rust exact pinned release (e.g., `1.85.0` in `rust-toolchain.toml` with `clippy` + `rustfmt`, installed strictly in `<repo>/.toolchain/`)

**Primary Dependencies**:
- `langflip-core`: `serde`, `toml` (zero OS-specific crates)
- `langflip-platform-win`: `windows` (official Microsoft Win32 bindings, minimal features)
- `langflip-app`: `langflip-core`, `langflip-platform-win` (Windows target), `ctrlc`

**Storage**: Optional local TOML configuration file (`langflip.toml`); zero persistence of keystrokes (volatile RAM only).

**Testing**: Strict TDD via `cargo test` running natively on Linux (`x86_64-unknown-linux-gnu`) with mocked platform traits; early Windows 11 PoC + full Windows 11 smoke/timing checklist for the thin Win32 adapter.

**Target Platform**: Windows 11 (`x86_64-pc-windows-gnu`, cross-compiled from Linux with `--locked`); core crate compiles and tests natively on Linux and is architected for future macOS/Linux adapters.

**Project Type**: Multi-crate Rust workspace (core library + OS adapter library + desktop CLI/background binary).

**Performance Goals**: Hook callback latency < 1 ms (non-blocking `try_send` into bounded `SyncSender`); full 50-character conversion + confirmed layout switch < 150 ms (`replay_delay_ms = 0` default); idle shutdown < 500 ms via `WM_QUIT` wake-up.

**Constraints**: Zero global toolchain mutation (`~/.rustup` / `~/.cargo` untouched); hard memory bounds on both raw input queue (`256`) and keystroke buffer (`1..=512`, default `128`); no keystroke logging in release builds; no destructive erasure before confirmed layout switch.

**Scale/Scope**: Single-user desktop utility, ~1,200–1,800 LOC across 3 crates for MVP.

## Constitution Check

*GATE: Checked against [constitution.md](file:///home/omri/projects/langflip/.specify/memory/constitution.md) v1.1.0.*

- **I. Cross-Platform Core, Thin OS Adapters**: PASS — All buffering, reset rules, conversion orchestration, race-cancellation, and config parsing live in `langflip-core` with zero Windows imports or LANGID tables; Win32 hooks, `KLID`/`HKL` resolution, and `SendInput` live strictly in `langflip-platform-win`.
- **II. Incremental Complexity**: PASS — MVP implements `NaiveBurstPolicy` and foreground-window `FocusObserver`, preserving clean trait seams for Phase 2 (UIA focus-aware multi-line/cursor-aware buffer + auto layout discovery), Phase 3 (UIA text reading + selection fallback), and Phase 4 (tray UI + macOS/Linux).
- **III. Isolated & Reproducible Tooling**: PASS — `toolchain/deps.conf`, exact pinned `rust-toolchain.toml` (with `clippy`/`rustfmt`), and committed `Cargo.lock` declare every dependency; `scripts/setup-env.sh` and `scripts/cargo.sh` enforce `.toolchain/`.
- **IV. Strict Test-Driven Development (TDD — NON-NEGOTIABLE)**: PASS — Every core module (`layout`, `config`, `buffer`, `engine`) begins with failing tests verified to fail for a known reason before implementation.
- **V. Privacy, Responsiveness & Non-Intrusiveness**: PASS — Hard-bounded `SyncSender(256)` + hard-bounded `KeystrokeBuffer(1..=512)` strictly in RAM, non-blocking `try_send` hook callback, `dwExtraInfo` self-injection marker, `WM_QUIT` idle shutdown wake-up, no disk/network I/O for keystrokes.
- **VI. Reuse Over Re-Implementation (DRY)**: PASS — Single canonical `ConversionEngine`, shared `HotkeySpec` matching, and single `LayoutId` normalization shared across all adapters.

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
├── Cargo.lock                      # Committed lockfile for reproducible builds
├── rust-toolchain.toml             # Exact pinned Rust release + clippy/rustfmt + targets
├── langflip.example.toml           # Documented example config
├── toolchain/
│   └── deps.conf                   # Declarative manifest of all host/system deps
├── scripts/
│   ├── setup-env.sh                # Checks/installs declared deps + isolated Rust toolchain
│   └── cargo.sh                    # Wrapper enforcing isolated env vars
└── crates/
    ├── langflip-core/              # Platform-independent domain & traits
    │   ├── Cargo.toml
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── key.rs              # PhysicalKey, Modifiers, KeyKind, KeyEvent, InputEvent
    │   │   ├── layout.rs           # Opaque LayoutId & LayoutPair (no Windows LANGID tables)
    │   │   ├── config.rs           # HotkeySpec, AppConfig, TOML parsing & validation
    │   │   ├── ports.rs            # InputListener, ListenerShutdown, InputInjector, LayoutController, FocusObserver
    │   │   ├── buffer.rs           # KeystrokeBuffer + BufferPolicy trait + NaiveBurstPolicy
    │   │   └── engine.rs           # ConversionEngine, ConversionTicket, safe Switch->Confirm->Erase->Replay runner
    │   └── tests/
    │       └── engine_tests.rs     # End-to-end mocked integration tests
    ├── langflip-platform-win/      # Thin Windows 11 Win32 adapter
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── hook.rs             # WH_KEYBOARD_LL + WH_MOUSE_LL (bounded try_send, auto-repeat filter, WM_QUIT shutdown)
    │       ├── injector.rs         # SendInput batched/paced scan-code & Backspace injection + partial-write detection
    │       ├── layout.rs           # Exact KLID/HKL resolution, ambiguity warnings, WM_INPUTLANGCHANGEREQUEST + confirmation poll
    │       └── focus.rs            # Foreground HWND FocusObserver (Phase 2 UIA seam)
    └── langflip-app/               # Thin binary entry point
        ├── Cargo.toml
        └── src/
            └── main.rs             # Config load, layout validation warnings, event loop, idle Ctrl+C shutdown
```

**Structure Decision**: A 3-crate Cargo workspace enforces compile-time isolation between OS-independent logic (`langflip-core`) and Win32 bindings (`langflip-platform-win`), guaranteeing native Linux TDD and drop-in future OS backends.

## Complexity Tracking

No constitution violations. All 3 crates are directly justified by Principle I (compile-time OS isolation) and Principle IV (native Linux TDD without Windows dependencies).
