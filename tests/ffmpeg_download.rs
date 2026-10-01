//! The pinned Windows FFmpeg download (see docs/FFMPEG.md).
//!
//! Opt-in, needs the network (~108 MB): `cargo test --test ffmpeg_download -- --ignored`.
//! Run it after changing the pinned build. It downloads, verifies the SHA-256 and writes
//! `ffmpeg.exe` next to the test binary, exactly as the app's "Download ffmpeg" button does.

#[cfg(target_os = "windows")]
#[test]
#[ignore]
fn pinned_ffmpeg_downloads_and_verifies() {
    use sha2::{Digest, Sha256};

    skelform_lib::export_modal::fetch_ffmpeg().unwrap();

    let exe = skelform_lib::utils::bin_path().join("ffmpeg.exe");
    let hash: String = Sha256::digest(std::fs::read(&exe).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // bin/ffmpeg.exe of the pinned gyan.dev build (docs/FFMPEG.md)
    assert_eq!(
        hash,
        "7f1ee3c1abf1d6b18e62d0006002ed26204e68f850429ae633e4dd530eaa1014"
    );
    std::fs::remove_file(exe).unwrap();
}
