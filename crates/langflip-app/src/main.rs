fn main() {
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("langflip MVP currently targets Windows 11; run `cargo test` on Linux to execute core tests.");
    }

    #[cfg(target_os = "windows")]
    {
        println!("langflip: Windows 11 runtime not yet wired (Phase 4).");
    }
}
