#![cfg(target_os = "windows")]

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
mod vhd_test_utils;

fn unique_path(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock error")
        .as_nanos();
    temp_root().join(format!("block-duplicator-{name}-{nonce}.bin"))
}

#[test]
fn copies_file_source_to_block_sink_via_vhd() {
    const SIZE: usize = 8 * 1024 * 1024;
    let vhd = vhd_test_utils::create_mounted_vhd(SIZE as u64);
    let drive_path = vhd.physical_drive_path();

    let src_path = unique_path("file-src");
    let verify_path = unique_path("verify-dst");

    let input: Vec<u8> = (0..SIZE).map(|i| (i % 241) as u8).collect();
    fs::write(&src_path, &input).expect("write source");

    let exe = env!("CARGO_BIN_EXE_bd");
    let status = Command::new(exe)
        .arg("--src-kind")
        .arg("file")
        .arg("--src")
        .arg(&src_path)
        .arg("--dst-kind")
        .arg("block")
        .arg("--dst")
        .arg(&drive_path)
        .arg("--chunk-size")
        .arg("1048576")
        .status()
        .expect("run block-duplicator");
    assert!(status.success(), "file->block command failed");

    let verify_status = Command::new(exe)
        .arg("--src-kind")
        .arg("block")
        .arg("--src")
        .arg(&drive_path)
        .arg("--dst-kind")
        .arg("file")
        .arg("--dst")
        .arg(&verify_path)
        .arg("--chunk-size")
        .arg("1048576")
        .status()
        .expect("verify by reading block back to file");
    assert!(verify_status.success(), "block->file verify command failed");

    let output = fs::read(&verify_path).expect("read verify destination");
    assert_eq!(output, input);

    if !vhd_test_utils::keep_tmp() {
        let _ = fs::remove_file(&src_path);
        let _ = fs::remove_file(&verify_path);
    } else {
        eprintln!("Keeping test file: {}", src_path.display());
        eprintln!("Keeping test file: {}", verify_path.display());
    }
}

fn temp_root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("_block-duplicator");
    std::fs::create_dir_all(&dir).expect("create test temp root");
    dir
}
