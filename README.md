# LangFlip

A lightweight, cross-platform-architected Rust utility (initially targeting **Windows 11**) that converts accidentally mistyped text between configurable keyboard layouts (default: English ↔ Hebrew) in-place across browsers, native apps, and command-line terminals on a single hotkey (`Pause/Break` by default).

## Highlights
- **Safe conversion ordering**: Switches and confirms the target layout *before* erasing a single character, and cancels cleanly if focus or input changes.
- **Works in terminals too**: Uses physical scan-code replay (not clipboard swap), so it works in Notepad, Chrome/Edge, and Windows Terminal / PowerShell / CMD.
- **Configurable languages & hotkeys**: Defaults to `en` ↔ `he` and `Pause`, customizable via [langflip.example.toml](langflip.example.toml).
- **Privacy by design**: Bounded RAM-only buffer with redacted `Debug`; keystrokes are never logged or persisted to disk.
- **Isolated reproducible toolchain**: All host dependencies, exact Rust `1.100.0`, and Windows cross-compilation (`x86_64-pc-windows-gnu`) are configured in `toolchain/deps.conf` and `rust-toolchain.toml`, installed strictly under `.toolchain/`.

## Quickstart
See [specs/001-mvp-layout-converter/quickstart.md](specs/001-mvp-layout-converter/quickstart.md) for isolated setup, native Linux TDD runs, Windows 11 cross-compilation, and the Windows 11 verification checklist.

## Roadmap
- **Phase 1 (MVP)**: Naive straight-line burst conversion, configurable layouts & hotkey, Windows 11 adapter.
- **Phase 2**: Windows UI Automation focus-aware multi-line & cursor-aware buffers + automatic OS layout discovery.
- **Phase 3**: Direct UIA text/caret reading, multi-character ligature key support, and selection-based fallback.
- **Phase 4**: System tray UI, logon auto-start, and macOS / Linux platform adapters reusing `langflip-core`.
