//! LangFlip desktop entry point (`T031`).

use std::path::PathBuf;
use std::process::ExitCode;

use langflip_core::config::AppConfig;

fn main() -> ExitCode {
    let metrics = std::env::args().any(|a| a == "--metrics");

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

    if let Some(path) = loaded_path {
        println!("langflip: loaded config from {}", path.display());
    } else {
        println!("langflip: using built-in defaults (no langflip.toml found)");
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (config, metrics);
        eprintln!(
            "langflip: MVP targets Windows 11. Run `./scripts/cargo.sh test --workspace --locked` on Linux for core tests."
        );
        ExitCode::SUCCESS
    }

    #[cfg(target_os = "windows")]
    {
        win_main::run(config, metrics)
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
    use std::process::ExitCode;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{self, sync_channel};
    use std::thread;
    use std::time::Instant;

    use langflip_core::config::AppConfig;
    use langflip_core::engine::{ConversionEngine, EngineOutcome};
    use langflip_core::layout::resolve_layouts;
    use langflip_core::ports::{InputListener, LayoutController, ListenerShutdown};
    use langflip_platform_win::focus::WinFocusObserver;
    use langflip_platform_win::hook::WinInputListener;
    use langflip_platform_win::injector::WinInputInjector;
    use langflip_platform_win::layout::WinLayoutController;

    const EVENT_QUEUE_CAPACITY: usize = 256;

    pub fn run(mut config: AppConfig, metrics: bool) -> ExitCode {
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

        println!(
            "langflip: active layout pair: {} <-> {} (max_buffer={}, replay_delay={}ms)",
            config.layouts.primary,
            config.layouts.secondary,
            config.max_buffer_len,
            config.replay_delay_ms
        );
        println!("langflip: listening... press Ctrl+C to exit.");

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

        let shutdown_for_ctrlc = shutdown_handle.clone();
        if let Err(err) = ctrlc::set_handler(move || {
            shutdown_for_ctrlc.stop();
        }) {
            eprintln!("langflip: failed to register Ctrl+C handler: {err}");
        }

        let mut engine = ConversionEngine::new(config, overflow_flag, latest_seq);
        let mut injector = WinInputInjector;
        let focus = WinFocusObserver;

        while let Ok(event) = rx.recv() {
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

        shutdown_handle.stop();
        let _ = hook_thread.join();
        println!("langflip: exited cleanly.");
        ExitCode::SUCCESS
    }
}
