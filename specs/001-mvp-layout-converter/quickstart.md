# Quickstart: LangFlip Isolated Dev Environment & Verification

**Feature**: `001-mvp-layout-converter` | **Date**: 2026-11-25 (Revised after Design Review)

## 1. Check & Bootstrap All Project Dependencies (Fresh Clone)
All required host tools (`curl`, `gcc`/`build-essential`, `x86_64-w64-mingw32-gcc`/`mingw-w64`, `ca-certificates`), the exact pinned Rust release (`rust-toolchain.toml` with `clippy` + `rustfmt`), and target triples are declared in version-controlled project configuration.

Audit what is installed vs. missing on your machine:
```bash
./scripts/setup-env.sh --check
```

Install any missing system dependencies + bootstrap the isolated Rust toolchain strictly inside `<repo>/.toolchain/` (without touching `~/.rustup` or `~/.cargo`):
```bash
./scripts/setup-env.sh --install-system-deps
```

*(If all system dependencies are already installed, run `./scripts/setup-env.sh` with no flags.)*

Verify isolation:
```bash
ls -la .toolchain/
# ~/.rustup and ~/.cargo remain untouched
```

## 2. Run Core Tests & Lints Natively on Linux (TDD Workflow)
```bash
./scripts/cargo.sh test --workspace --locked
./scripts/cargo.sh clippy --workspace --all-targets --locked -- -D warnings
```

## 3. Cross-Compile Windows 11 Release Binary
```bash
./scripts/cargo.sh build --release --locked --target x86_64-pc-windows-gnu -p langflip-app
# Output: target/x86_64-pc-windows-gnu/release/langflip.exe
```

## 4. Windows 11 Verification Checklist
Copy `langflip.exe` (and optional `langflip.toml`) to your Windows 11 machine, ensure both configured layouts (default: US QWERTY `"00000409"` and Israeli Hebrew Standard `"0000040D"`) are installed, and run `langflip.exe`:

1. **Notepad (HE → EN)**: Switch to Hebrew, type `"יקךךם 'םרךג"`, press `Pause/Break` → becomes `"hello world"`, layout switches to English, continuing to type produces English.
2. **Reverse direction (EN → HE)**: In English layout, type `"akuo guko"`, press `Pause/Break` → becomes `"שלום עולם"`, layout switches to Hebrew.
3. **Round-trip double-tap vs. hold**: Press, release, and press `Pause/Break` again → converts cleanly back to `"akuo guko"` and English. Hold `Pause/Break` down for 2 seconds → triggers only once (auto-repeat ignored).
4. **Browser (Chrome/Edge)**: Repeat in a web `<textarea>` and address bar.
5. **Command Line (Windows Terminal / PowerShell / CMD)**: Switch to Hebrew, type `"גןר"` (physical keys for `"dir"`), press `Pause/Break` → becomes `"dir"`, press `Enter` → executes cleanly.
6. **Reset boundary safety**: Type `"hello"`, press `Enter` (or click elsewhere, or press `Alt+Shift` to change layout externally), type `"יקךךם"`, press `Pause/Break` → only `"יקךךם"` is converted; `"hello"` is untouched.
7. **Custom hotkey (`langflip.toml`)**: Bind `Ctrl+Shift+L` in `langflip.toml`, restart `langflip.exe`, verify `Ctrl+Shift+L` converts cleanly without stuck modifiers and `Pause/Break` passes through normally.
8. **Missing / ambiguous layout warning**: Configure a non-installed layout ID in `langflip.toml`, start `langflip.exe`, verify a clear startup warning is printed and no text is corrupted.
9. **Elevated (Administrator) window safety**: Open an elevated PowerShell while running `langflip.exe` non-elevated, type `"גןר"`, press `Pause/Break` → layout switch is rejected/unconfirmed, **zero Backspaces are sent**, and `"גןר"` remains intact.
10. **Performance & latency**: Run with `--metrics` (or debug timing log) on a 50-character burst → confirm end-to-end conversion < 150 ms and hook callback duration < 1 ms.
11. **Idle graceful shutdown**: Leave the machine idle (no mouse/keyboard input for 5 seconds) and press `Ctrl+C` in the `langflip.exe` console → exits cleanly within 500 ms with all hooks removed.
