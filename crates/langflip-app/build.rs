use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=langflip.exe.manifest");
    println!("cargo:rerun-if-changed=langflip.rc");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let res_obj = out_dir.join("langflip_manifest.o");

    let windres = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        "x86_64-w64-mingw32-windres"
    } else {
        "windres"
    };

    let status = Command::new(windres)
        .args(["langflip.rc", "-O", "coff", "-o"])
        .arg(&res_obj)
        .status()
        .unwrap_or_else(|e| panic!("failed to run `{windres}` for Windows manifest: {e}"));

    assert!(status.success(), "`{windres}` failed compiling langflip.rc");
    println!("cargo:rustc-link-arg={}", res_obj.display());
}
