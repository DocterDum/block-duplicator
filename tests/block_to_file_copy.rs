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
fn copies_block_source_to_file_sink_via_vhd() {
    const SIZE: usize = 8 * 1024 * 1024;
    let vhd = vhd_test_utils::create_mounted_vhd(SIZE as u64);
    let drive_path = vhd.physical_drive_path();

    let seed_path = unique_path("seed-src");
    let dst_path = unique_path("block-dst");
    let input: Vec<u8> = (0..SIZE).map(|i| (i % 253) as u8).collect();
    fs::write(&seed_path, &input).expect("write seed source");

    let exe = env!("CARGO_BIN_EXE_bd");
    let write_status = Command::new(exe)
        .arg("--src-kind")
        .arg("file")
        .arg("--src")
        .arg(&seed_path)
        .arg("--dst-kind")
        .arg("block")
        .arg("--dst")
        .arg(&drive_path)
        .arg("--chunk-size")
        .arg("1048576")
        .status()
        .expect("seed VHD from file");
    assert!(write_status.success(), "seed command failed");

    let read_status = Command::new(exe)
        .arg("--src-kind")
        .arg("block")
        .arg("--src")
        .arg(&drive_path)
        .arg("--dst-kind")
        .arg("file")
        .arg("--dst")
        .arg(&dst_path)
        .arg("--chunk-size")
        .arg("1048576")
        .status()
        .expect("run block-duplicator");
    assert!(read_status.success(), "block->file command failed");

    let output = fs::read(&dst_path).expect("read destination");
    assert_eq!(output, input);

    if !vhd_test_utils::keep_tmp() {
        let _ = fs::remove_file(&seed_path);
        let _ = fs::remove_file(&dst_path);
    } else {
        eprintln!("Keeping test file: {}", seed_path.display());
        eprintln!("Keeping test file: {}", dst_path.display());
    }
}

fn temp_root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("_block-duplicator");
    std::fs::create_dir_all(&dir).expect("create test temp root");
    dir
}
