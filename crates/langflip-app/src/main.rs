//! LangFlip desktop entry point (`T011`).
//!
//! Runs as a background Windows GUI-subsystem tray utility (zero console window
//! on normal launch; attaches to parent console if `--metrics` is passed).

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use langflip_core::config::AppConfig;

fn main() -> ExitCode {
    let metrics = std::env::args().any(|a| a == "--metrics");

    #[cfg(target_os = "windows")]
    if metrics {
        langflip_platform_win::tray::attach_parent_console();
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let os_config_dir = os_config_dir();

    let (config, loaded_path) = match AppConfig::load_with_precedence(
        exe_dir.as_deref(),
        os_config_dir.as_deref(),
    ) {
        Ok(res) => res,
        Err(err) => {
            eprintln!("langflip: configuration error: {err}");
            return ExitCode::FAILURE;
        }
    };

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (config, loaded_path, metrics);
        eprintln!(
            "langflip: MVP targets Windows 11. Run `./scripts/cargo.sh test --workspace --locked` on Linux for core tests."
        );
        ExitCode::SUCCESS
    }

    #[cfg(target_os = "windows")]
    {
        win_main::run(config, loaded_path, os_config_dir, metrics)
    }
}

fn os_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }
}

#[cfg(target_os = "windows")]
mod win_main {
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{self, sync_channel};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Instant;

    use langflip_core::config::AppConfig;
    use langflip_core::engine::{ConversionEngine, EngineOutcome};
    use langflip_core::hotkey::HotkeySpec;
    use langflip_core::layout::{LayoutPair, resolve_layouts};
    use langflip_core::ports::{
        InputListener, LayoutController, ListenerShutdown, SettingsSnapshot, TrayHost,
    };
    use langflip_platform_win::autostart::sync_auto_start;
    use langflip_platform_win::focus::WinFocusObserver;
    use langflip_platform_win::hook::WinInputListener;
    use langflip_platform_win::injector::WinInputInjector;
    use langflip_platform_win::layout::WinLayoutController;
    use langflip_platform_win::tray::{SingleInstanceGuard, WinTrayHost};

    const EVENT_QUEUE_CAPACITY: usize = 256;

    pub fn run(
        mut config: AppConfig,
        loaded_path: Option<PathBuf>,
        os_config_dir: Option<PathBuf>,
        metrics: bool,
    ) -> ExitCode {
        let _single_instance = match SingleInstanceGuard::acquire() {
            Ok(Some(guard)) => guard,
            Ok(None) => {
                if metrics {
                    eprintln!("langflip: already running.");
                }
                return ExitCode::SUCCESS;
            }
            Err(err) => {
                eprintln!("langflip: mutex error: {err}");
                return ExitCode::FAILURE;
            }
        };

        let mut layouts = WinLayoutController::new();
        let installed = match layouts.list_installed_layouts() {
            Ok(list) => list,
            Err(err) => {
                eprintln!("langflip: failed to enumerate installed layouts: {err}");
                return ExitCode::FAILURE;
            }
        };

        let (resolved_pair, warnings) = resolve_layouts(&installed, &config.layouts);
        for w in warnings {
            eprintln!("langflip: warning: {}", w.message);
        }
        config.layouts = resolved_pair;

        if let Err(err) = sync_auto_start(config.auto_start) {
            eprintln!("langflip: warning: {err}");
        }

        if metrics {
            println!(
                "langflip: active layout pair: {} <-> {} (max_buffer={}, replay_delay={}ms)",
                config.layouts.primary,
                config.layouts.secondary,
                config.max_buffer_len,
                config.replay_delay_ms
            );
        }

        let (tx, rx) = sync_channel(EVENT_QUEUE_CAPACITY);
        let overflow_flag = Arc::new(AtomicBool::new(false));
        let latest_seq = Arc::new(AtomicU64::new(0));
        let max_callback_us = Arc::new(AtomicU64::new(0));

        let listener = WinInputListener::new(layouts.hkl_map_snapshot(), max_callback_us.clone());
        let hotkey = config.hotkey;
        let (ready_tx, ready_rx) = mpsc::channel();

        let overflow_for_hook = overflow_flag.clone();
        let seq_for_hook = latest_seq.clone();
        let hook_thread = thread::spawn(move || {
            if let Err(err) = listener.run(
                hotkey,
                tx,
                overflow_for_hook,
                seq_for_hook,
                move |shutdown| {
                    let _ = ready_tx.send(shutdown);
                },
            ) {
                eprintln!("langflip: input listener error: {err}");
            }
        });

        let Ok(shutdown_handle) = ready_rx.recv() else {
            eprintln!("langflip: input listener failed to initialize");
            return ExitCode::FAILURE;
        };

        // Shared live-config update slot read by the worker thread on each event.
        let pending_update: Arc<Mutex<Option<(LayoutPair, HotkeySpec)>>> =
            Arc::new(Mutex::new(None));
        let pending_for_worker = pending_update.clone();

        let worker_config = config.clone();
        let worker_thread = thread::spawn(move || {
            let mut engine = ConversionEngine::new(worker_config, overflow_flag, latest_seq);
            let mut injector = WinInputInjector;
            let focus = WinFocusObserver;

            while let Ok(event) = rx.recv() {
                if let Ok(mut guard) = pending_for_worker.lock()
                    && let Some((new_pair, new_hotkey)) = guard.take()
                {
                    engine.update_config(new_pair, new_hotkey);
                }

                let start = Instant::now();
                let outcome = engine.handle_event(event, &mut injector, &mut layouts, &focus);
                match outcome {
                    EngineOutcome::Converted {
                        erased,
                        replayed,
                        new_layout,
                    } if metrics => {
                        println!(
                            "[metrics] converted: erased={erased} replayed={replayed} layout={new_layout} elapsed={}ms max_hook_us={}",
                            start.elapsed().as_millis(),
                            max_callback_us.load(Ordering::Relaxed)
                        );
                    }
                    EngineOutcome::LayoutToggled(new_layout) if metrics => {
                        println!(
                            "[metrics] toggled layout -> {new_layout} elapsed={}ms",
                            start.elapsed().as_millis()
                        );
                    }
                    EngineOutcome::AbortedSwitchRejected if metrics => {
                        println!(
                            "[metrics] aborted: layout switch rejected/timed out elapsed={}ms",
                            start.elapsed().as_millis()
                        );
                    }
                    EngineOutcome::AbortedStale if metrics => {
                        println!(
                            "[metrics] aborted: stale focus/sequence elapsed={}ms",
                            start.elapsed().as_millis()
                        );
                    }
                    EngineOutcome::AbortedThirdLayout if metrics => {
                        println!(
                            "[metrics] aborted: active layout not in configured pair elapsed={}ms",
                            start.elapsed().as_millis()
                        );
                    }
                    EngineOutcome::FailedInjection(err) => {
                        eprintln!("langflip: warning: {err}");
                    }
                    EngineOutcome::LayoutError(err) => {
                        eprintln!("langflip: warning: layout error: {err}");
                    }
                    _ => {}
                }
            }
        });

        let initial_snapshot = SettingsSnapshot {
            installed_layouts: installed,
            layouts: config.layouts.clone(),
            hotkey: config.hotkey,
            auto_start: config.auto_start,
        };

        let mut active_config = config;
        let shutdown_for_save = shutdown_handle.clone();
        let shutdown_for_exit = shutdown_handle.clone();

        let tray_res = WinTrayHost.run(
            initial_snapshot,
            move |new_pair, new_hotkey, new_auto_start| {
                let mut updated = active_config.clone();
                updated.layouts = new_pair.clone();
                updated.hotkey = new_hotkey;
                updated.auto_start = new_auto_start;

                let save_path = AppConfig::resolve_save_path(
                    loaded_path.as_deref(),
                    os_config_dir.as_deref(),
                )
                .map_err(|e| e.to_string())?;
                updated.save_to_path(&save_path).map_err(|e| e.to_string())?;
                sync_auto_start(new_auto_start)?;

                active_config = updated;
                if let Ok(mut guard) = pending_update.lock() {
                    *guard = Some((new_pair, new_hotkey));
                }
                shutdown_for_save.update_hotkey(new_hotkey);
                Ok(())
            },
            move || {
                shutdown_for_exit.stop();
            },
        );

        shutdown_handle.stop();
        let _ = hook_thread.join();
        let _ = worker_thread.join();

        if let Err(err) = tray_res {
            eprintln!("langflip: tray error: {err}");
            return ExitCode::FAILURE;
        }
        ExitCode::SUCCESS
    }
}
