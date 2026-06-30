#![cfg(target_os = "windows")]

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

mod vhd_test_utils;

fn unique_path(name: &str, ext: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock error")
        .as_nanos();
    temp_root().join(format!("block-duplicator-{name}-{nonce}.{ext}"))
}

#[test]
fn copies_block_source_to_vhdx_sink_via_binary() {
    const SIZE: usize = 8 * 1024 * 1024;

    let src_vhd = vhd_test_utils::create_mounted_vhd(SIZE as u64);
    let src_drive_path = src_vhd.physical_drive_path();
    let dst_vhdx_path = unique_path("dst-vhdx", "vhdx");
    let seed_path = unique_path("seed-src", "bin");
    let verify_path = unique_path("verify-dst", "bin");

    let input: Vec<u8> = (0..SIZE).map(|i| (i % 233) as u8).collect();
    fs::write(&seed_path, &input).expect("write seed source");

    let exe = env!("CARGO_BIN_EXE_bd");

    let seed_status = Command::new(exe)
        .arg("--src-kind")
        .arg("file")
        .arg("--src")
        .arg(&seed_path)
        .arg("--dst-kind")
        .arg("block")
        .arg("--dst")
        .arg(&src_drive_path)
        .arg("--chunk-size")
        .arg("1048576")
        .status()
        .expect("seed source block device");
    assert!(seed_status.success(), "seed command failed");

    let copy_status = Command::new(exe)
        .arg("--src-kind")
        .arg("block")
        .arg("--src")
        .arg(&src_drive_path)
        .arg("--dst-kind")
        .arg("vhdx")
        .arg("--dst")
        .arg(&dst_vhdx_path)
        .arg("--vhdx-size-bytes")
        .arg(SIZE.to_string())
        .arg("--chunk-size")
        .arg("1048576")
        .status()
        .expect("copy block source to vhdx");
    assert!(copy_status.success(), "block->vhdx command failed");

    let mounted_dst = vhd_test_utils::mount_existing_vhd(&dst_vhdx_path);
    let dst_drive_path = mounted_dst.physical_drive_path();

    let verify_status = Command::new(exe)
        .arg("--src-kind")
        .arg("block")
        .arg("--src")
        .arg(&dst_drive_path)
        .arg("--dst-kind")
        .arg("file")
        .arg("--dst")
        .arg(&verify_path)
        .arg("--chunk-size")
        .arg("1048576")
        .status()
        .expect("verify copied VHDX by reading back");
    assert!(verify_status.success(), "verify command failed");

    let output = fs::read(&verify_path).expect("read verify destination");
    assert_eq!(output, input);

    drop(mounted_dst);
    if !vhd_test_utils::keep_tmp() {
        let _ = fs::remove_file(&dst_vhdx_path);
        let _ = fs::remove_file(&seed_path);
        let _ = fs::remove_file(&verify_path);
    } else {
        eprintln!("Keeping test file: {}", dst_vhdx_path.display());
        eprintln!("Keeping test file: {}", seed_path.display());
        eprintln!("Keeping test file: {}", verify_path.display());
    }
}

fn temp_root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("_block-duplicator");
    std::fs::create_dir_all(&dir).expect("create test temp root");
    dir
}
