//! Sets `cfg(rtsp_fixture)` / `cfg(video_fixture)` once recorded fixtures exist, so the replay tests
//! run then and are reported as ignored (not passed) before.
fn has(dir: &str, suffix: &str) -> bool {
    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join(dir);
    println!("cargo::rerun-if-changed={}", dir.display());
    std::fs::read_dir(&dir)
        .map(|d| {
            d.flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with(suffix))
        })
        .unwrap_or(false)
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(rtsp_fixture)");
    println!("cargo::rustc-check-cfg=cfg(video_fixture)");
    if has("../../fixtures/rtsp", "describe-resp.bin") {
        println!("cargo::rustc-cfg=rtsp_fixture");
    }
    if has("../../fixtures/video", ".bin") {
        println!("cargo::rustc-cfg=video_fixture");
    }
}
