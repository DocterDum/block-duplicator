use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_path(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock error")
        .as_nanos();
    temp_root().join(format!("block-duplicator-{name}-{nonce}.bin"))
}

#[test]
fn copies_file_contents_end_to_end() {
    let src_path = unique_path("src");
    let dst_path = unique_path("dst");

    let input: Vec<u8> = (0..(1024 * 1024 + 123))
        .map(|i| (i % 251) as u8)
        .collect();
    fs::write(&src_path, &input).expect("write source file");

    let exe = env!("CARGO_BIN_EXE_bd");
    let status = Command::new(exe)
        .arg("--src")
        .arg(&src_path)
        .arg("--dst")
        .arg(&dst_path)
        .arg("--chunk-size")
        .arg("65536")
        .status()
        .expect("run block-duplicator");

    assert!(status.success(), "copy command failed");

    let output = fs::read(&dst_path).expect("read destination file");
    assert_eq!(output, input, "destination bytes differ from source");

    if keep_tmp() {
        eprintln!("Keeping test file: {}", src_path.display());
        eprintln!("Keeping test file: {}", dst_path.display());
    } else {
        let _ = fs::remove_file(&src_path);
        let _ = fs::remove_file(&dst_path);
    }
}

fn keep_tmp() -> bool {
    matches!(std::env::var("BLOCK_DUP_KEEP_TMP").ok().as_deref(), Some("1"))
}

fn temp_root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("_block-duplicator");
    std::fs::create_dir_all(&dir).expect("create test temp root");
    dir
}
