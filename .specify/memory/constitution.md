# LangFlip Constitution

## Core Principles

### I. Cross-Platform Core, Thin OS Adapters
All domain logic — keystroke buffering, cursor/reset state machines, physical-key-to-layout conversion, configuration, and conversion orchestration — MUST live in platform-independent Rust modules communicating with the operating system exclusively through narrow traits (e.g., `InputListener`, `InputInjector`, `LayoutController`, `FocusObserver`). OS-specific code (Windows 11 first; macOS and Linux later) MUST be isolated behind those trait implementations and gated with `#[cfg(target_os = "...")]`.

### II. Incremental Complexity (Naive First, Extensible by Design)
Every capability starts with the simplest, lowest-dependency working slice (MVP) and grows through clean extension points. Architectural seams (e.g., pluggable buffer strategies, optional focus/UI-automation observers, optional selection fallback) MUST be designed upfront so future phases (multi-line buffers, UIA caret reading, tray UI, additional languages) add modules rather than rewriting the core.

### III. Isolated & Reproducible Tooling
The Rust development environment (`RUSTUP_HOME`, `CARGO_HOME`, target toolchains, and wrapper scripts) MUST be installed in a project-local isolated directory (`.toolchain/`) and MUST NOT mutate the user's global `~/.rustup` or `~/.cargo` directories. Builds MUST be reproducible from documented setup scripts.

### IV. Strict Test-Driven Development (TDD — NON-NEGOTIABLE)
All platform-independent core logic (buffer state transitions, reset triggers, scan-code/character mapping, replay sequence generation, config loading) MUST follow strict Red-Green-Refactor TDD:
- Before writing any production code, first write failing tests that clearly illustrate the intended behavior of the code.
- Verify that the new tests fail for the expected, known reason before implementing.
- Implement and evolve the production code until those tests turn green, then refactor while keeping tests passing.
- All core unit and integration tests MUST execute natively on the development host (`cargo test`) without requiring a Windows runtime or GUI session. OS adapter layers MUST be kept thin and verified with targeted manual/smoke test checklists on the target OS.

### V. Privacy, Responsiveness & Non-Intrusiveness
Because a global keyboard hook observes all keystrokes:
- Keystroke buffers MUST stay strictly in volatile RAM with a hard upper bound and MUST NEVER be written to disk, network, logs (debug or release), or panic/`Debug` output.
- Low-level input hook callbacks MUST be non-blocking (enqueue-only) so the utility never lags or freezes user typing.
- Normal typing MUST pass through unmodified unless the configured hotkey is pressed.

### VI. Reuse Over Re-Implementation (DRY)
Prefer reusing existing code over re-implementing logic. Before writing a new function or module, check whether the capability — or something closely similar — already exists in the codebase. When two or more places need similar behavior, extract a clean, shared core function or helper and call it from all callers rather than duplicating logic.

## Technology & Platform Constraints

- **Language**: Rust (stable toolchain pinned in `rust-toolchain.toml`).
- **Primary Target (v1)**: Windows 11 (`x86_64-pc-windows-gnu`), cross-compiled from Linux and tested on Windows 11.
- **Future Targets**: macOS and Linux (Wayland/X11) via new platform adapter implementations without modifying the core crate.
- **Dependencies**: Minimize external crate dependencies in the naive MVP; prefer official bindings (`windows` crate on Windows) and standard library primitives.

## Development Workflow

- Follow the Spec Kit lifecycle: **Specify -> Clarify -> Plan -> Tasks -> Analyze -> Implement -> Converge**.
- All feature work happens on numbered feature branches (`###-short-name`) created via `.specify/scripts/bash/create-new-feature.sh`.
- Specs stay implementation-agnostic in user stories and requirements; architecture, crates, and module layouts belong in `plan.md`.

## Governance

This constitution supersedes ad-hoc practices for the LangFlip repository. Amendments require updating this file, incrementing the version, and recording the rationale.

**Version**: 1.1.1 | **Ratified**: 2026-11-25 | **Last Amended**: 2026-11-25
