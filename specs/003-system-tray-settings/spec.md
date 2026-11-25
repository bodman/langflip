# Feature Specification: LangFlip — System Tray Icon, Context Menu & Settings Dialog

**Feature Branch**: `003-system-tray-settings`

**Created**: 2026-11-26

**Status**: Draft

**Input**: User description: "1. Run LangFlip in the Windows taskbar notification area (system tray) like a native background utility with a clean icon and no lingering console window. 2. Right-clicking the tray icon shows a context menu with two options: Settings and Exit. 3. Selecting Settings opens a small dialog where the user can configure the two keyboard languages and the trigger hotkey."

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Background System Tray Utility with Clean Icon & Exit (Priority: P1)

As a Windows 11 user, when I launch LangFlip, I want it to run quietly in the taskbar notification area (system tray) with a recognizable custom icon and **no open console window**, and I want right-clicking the icon to open a menu with **Settings** and **Exit** so I can cleanly stop LangFlip at any time.

**Why this priority**: Transforms LangFlip from a developer console tool into an everyday Windows utility that stays out of the way.

**Independent Test**: Launch `langflip.exe` on Windows 11 by double-clicking it (or from Explorer/Run), verify no console window opens, verify the custom LangFlip icon and tooltip appear in the system tray, verify right-clicking displays **Settings** and **Exit**, verify mistype conversion works normally while running in the tray, and verify clicking **Exit** removes the tray icon, unhooks all OS hooks, and terminates the process cleanly.

**Acceptance Scenarios**:

1. **Given** LangFlip is not running, **When** the user launches `langflip.exe`, **Then** no console window is shown, a crisp LangFlip icon appears in the Windows taskbar notification area with a tooltip showing the app name, and background mistype conversion is immediately active.
2. **Given** LangFlip is already running in the system tray, **When** the user launches a second copy of `langflip.exe`, **Then** the second instance exits cleanly without creating a duplicate tray icon or duplicate input hooks.
3. **Given** LangFlip is running in the system tray, **When** the user right-clicks the tray icon, **Then** a native context menu appears with exactly two items: **Settings** and **Exit**.
4. **Given** the tray context menu is open, **When** the user clicks **Exit**, **Then** the tray icon is removed immediately, all OS input hooks are cleanly torn down, and the process exits cleanly within 500 ms.

---

### User Story 2 - Settings Dialog for Languages & Hotkey with Live Apply and Persistence (Priority: P1)

As a user, when I right-click the LangFlip tray icon and choose **Settings**, I want a small, clean dialog where I can pick my two keyboard languages from the layouts installed on my system and choose my preferred trigger hotkey (key + optional `Ctrl`/`Alt`/`Shift`/`Win` modifiers), click **Save**, and have the new settings take effect immediately and persist across restarts.

**Why this priority**: Eliminates the need to manually edit `langflip.toml` or look up OS layout IDs to customize languages or hotkeys.

**Independent Test**:
1. Core TDD tests on Linux verify config serialization (`to_toml`), round-trip parse/save fidelity (preserving existing behavior fields), hotkey key-name reverse lookup, validation (rejecting identical primary/secondary languages), and live config update inside the engine.
2. On Windows 11, open **Settings** from the tray menu, change the hotkey (e.g., `Pause` → `F9` or `Ctrl+Shift+L`) and/or swap/select installed languages, click **Save**, verify the new hotkey works immediately without restarting, and restart `langflip.exe` to confirm the saved settings reload automatically.

**Acceptance Scenarios**:

1. **Given** LangFlip is running in the tray, **When** the user clicks **Settings**, **Then** a compact dialog opens displaying:
   - **Primary Language** dropdown populated with the OS's installed keyboard layouts (pre-selecting the current primary layout).
   - **Secondary Language** dropdown populated with the OS's installed keyboard layouts (pre-selecting the current secondary layout).
   - **Hotkey** selector showing the current physical key (e.g., `Pause`) and modifier checkboxes (`Ctrl`, `Alt`, `Shift`, `Win`).
   - **Save** and **Cancel** buttons.
2. **Given** the Settings dialog is already open, **When** the user clicks **Settings** again from the tray menu, **Then** the existing Settings dialog is brought to the foreground rather than opening a second window.
3. **Given** the user selects two identical languages and clicks **Save**, **Then** LangFlip rejects the change with a clear validation message and keeps the dialog open without corrupting the saved config.
4. **Given** the user selects valid languages and a new hotkey and clicks **Save**, **Then** the settings are persisted to `langflip.toml` (preserving other settings such as `max_buffer_len` and `replay_delay_ms`), the dialog closes, and the new languages and hotkey take effect **immediately** without restarting LangFlip.
5. **Given** the user modifies fields in the Settings dialog and clicks **Cancel** (or closes the dialog with `X`), **Then** no changes are saved or applied and LangFlip continues using its previous settings.

---

### Edge Cases

- **While Settings dialog is open**: Normal mistype conversion MUST NOT interfere with interacting with the Settings dialog (and clicking/changing focus to the dialog resets any prior typing burst as usual).
- **Saving when no `langflip.toml` existed yet**: If LangFlip started with built-in defaults, saving from the Settings dialog MUST create `langflip.toml` in the standard user config location (creating parent directories if needed). If LangFlip loaded an existing `langflip.toml` (e.g., portable file next to the executable), saving MUST update that same file.
- **Preserving non-dialog settings**: Saving from the Settings dialog MUST preserve `max_buffer_len`, `replay_delay_ms`, and `toggle_on_empty_buffer` from the active config rather than resetting them to defaults.
- **Taskbar restart (`Explorer.exe` crash/restart)**: When Windows Explorer restarts the taskbar, LangFlip SHOULD re-register its tray icon automatically so the icon does not silently disappear.
- **Clicking outside the tray context menu**: Clicking anywhere outside the right-click popup menu MUST dismiss the menu cleanly (standard Windows tray behavior).
- **High-DPI displays**: The tray icon and Settings dialog MUST render crisply on Windows 11 scaling (100%–200% DPI).

---

## Requirements *(mandatory)*

### Functional Requirements

#### Core Config Serialization & Live Reload (Cross-Platform, Strict TDD)
- **FR-001**: The core configuration module MUST support reverse-mapping a `PhysicalKey` back to its canonical key name (e.g., `Pause`, `F9`, `L`) and serializing a validated `AppConfig` back to valid TOML (`to_toml()`), preserving all fields.
- **FR-002**: The core configuration module MUST provide a deterministic save target resolver (updating the loaded config file if one was loaded, otherwise creating `langflip.toml` in the OS user config directory) and atomic/safe file write.
- **FR-003**: The core conversion engine and hotkey matcher MUST support live-updating the active `LayoutPair` and `HotkeySpec` at runtime (clearing any in-flight burst on update) without requiring a process restart.

#### Windows 11 System Tray & Context Menu
- **FR-004**: On Windows 11, LangFlip MUST run as a GUI-subsystem background application with no console window on normal launch.
- **FR-005**: LangFlip MUST enforce single-instance execution so launching it twice does not create duplicate tray icons or duplicate hooks.
- **FR-006**: LangFlip MUST display a custom, recognizable tray icon in the Windows taskbar notification area with a tooltip, re-registering it if Windows Explorer restarts the taskbar.
- **FR-007**: Right-clicking the tray icon MUST open a native context menu with two options: **Settings** and **Exit**; clicking outside the menu MUST dismiss it cleanly.
- **FR-008**: Clicking **Exit** MUST remove the tray icon, wake and cleanly stop the input listener (unhooking all OS hooks), and exit the process cleanly.

#### Settings Dialog
- **FR-009**: Clicking **Settings** MUST open a compact native dialog (bringing the existing dialog to front if already open) allowing the user to configure:
  - Primary keyboard language (populated from installed OS layouts).
  - Secondary keyboard language (populated from installed OS layouts).
  - Trigger hotkey (canonical physical key list + `Ctrl`, `Alt`, `Shift`, `Win` checkboxes).
- **FR-010**: Clicking **Save** MUST validate the selections via core validation rules (rejecting identical languages with a clear message), persist the updated `langflip.toml` (preserving non-dialog settings), apply the new layout pair and hotkey live to both the input listener and conversion engine, and close the dialog.
- **FR-011**: Clicking **Cancel** or closing the dialog MUST discard unsaved changes without modifying `langflip.toml` or runtime state.

### Key Entities

- **ConfigUpdate**: Validated `(LayoutPair, HotkeySpec)` payload produced by the Settings dialog, persisted to `langflip.toml`, and applied live to the running hook matcher and conversion engine.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of core config serialization, key-name reverse lookup, round-trip TOML fidelity, save-path precedence, and live engine/hotkey reload tests pass natively on Linux via strict TDD.
- **SC-002**: Launching `langflip.exe` on Windows 11 opens zero console windows, shows the custom LangFlip icon in the system tray, and prevents duplicate instances.
- **SC-003**: Right-clicking the tray icon displays **Settings** and **Exit**; **Exit** removes the tray icon and terminates cleanly within 500 ms.
- **SC-004**: Changing languages and/or hotkey in the **Settings** dialog and clicking **Save** takes effect immediately (without restart), persists to `langflip.toml`, and reloads accurately on the next launch.
- **SC-005**: All existing MVP + Spec 002 behaviors (Notepad, Windows Terminal, Edge address bar, `< 1 ms` hook latency, RAM-only keystroke privacy) remain 100% intact.
