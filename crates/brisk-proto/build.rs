//! Sets `cfg(rtsp_fixture)` once a recorded DESCRIBE reply exists, so the replay test runs then and
//! is reported as ignored (not passed) before.
fn main() {
    let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../fixtures/rtsp");
    println!("cargo::rerun-if-changed={}", dir.display());
    println!("cargo::rustc-check-cfg=cfg(rtsp_fixture)");
    let found = std::fs::read_dir(&dir)
        .map(|d| {
            d.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with("describe-resp.bin")
            })
        })
        .unwrap_or(false);
    if found {
        println!("cargo::rustc-cfg=rtsp_fixture");
    }
}
