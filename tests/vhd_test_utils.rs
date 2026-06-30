#![cfg(target_os = "windows")]

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct MountedVhd {
    pub vhd_path: PathBuf,
    pub disk_number: u32,
    delete_on_drop: bool,
}

impl MountedVhd {
    pub fn physical_drive_path(&self) -> String {
        format!(r"\\.\PhysicalDrive{}", self.disk_number)
    }
}

impl Drop for MountedVhd {
    fn drop(&mut self) {
        let path = ps_single_quoted(&self.vhd_path.to_string_lossy());
        let _ = run_powershell(&format!(
            "Dismount-DiskImage -ImagePath '{path}' -ErrorAction SilentlyContinue"
        ));

        if keep_tmp() {
            eprintln!("Keeping VHD for inspection: {}", self.vhd_path.display());
            return;
        }
        if self.delete_on_drop {
            let _ = run_powershell(&format!(
                "Remove-Item -LiteralPath '{path}' -Force -ErrorAction SilentlyContinue"
            ));
        }
    }
}

pub fn create_mounted_vhd(size_bytes: u64) -> MountedVhd {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock error")
        .as_nanos();
    let vhd_path = temp_root().join(format!("block-duplicator-test-{nonce}.vhdx"));
    let size_mb = (size_bytes / (1024 * 1024)).max(8);
    let dp_script = format!(
        "create vdisk file=\"{}\" maximum={} type=expandable\nselect vdisk file=\"{}\"\nattach vdisk\n",
        vhd_path.display(),
        size_mb,
        vhd_path.display()
    );
    run_diskpart(&dp_script).expect("failed to create/attach VHD with diskpart (run tests in elevated shell)");

    let path = ps_single_quoted(&vhd_path.to_string_lossy());
    let out = run_powershell(&format!(
        "(Get-DiskImage -ImagePath '{path}' -ErrorAction Stop | Get-Disk -ErrorAction Stop).Number"
    ))
    .expect("failed to resolve mounted VHD disk number");
    let disk_number: u32 = out
        .trim()
        .parse()
        .expect("failed to parse mounted disk number");

    MountedVhd {
        vhd_path,
        disk_number,
        delete_on_drop: true,
    }
}

#[allow(dead_code)]
pub fn mount_existing_vhd(vhd_path: &std::path::Path) -> MountedVhd {
    let path = ps_single_quoted(&vhd_path.to_string_lossy());
    let _ = run_powershell(&format!(
        "Mount-DiskImage -ImagePath '{path}' -ErrorAction Stop"
    ))
    .expect("failed to mount existing VHD");

    let out = run_powershell(&format!(
        "(Get-DiskImage -ImagePath '{path}' -ErrorAction Stop | Get-Disk -ErrorAction Stop).Number"
    ))
    .expect("failed to resolve mounted VHD disk number");
    let disk_number: u32 = out
        .trim()
        .parse()
        .expect("failed to parse mounted disk number");

    MountedVhd {
        vhd_path: vhd_path.to_path_buf(),
        disk_number,
        delete_on_drop: false,
    }
}

fn run_powershell(script: &str) -> Result<String, String> {
    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .output()
        .map_err(|e| format!("failed to launch powershell: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn run_diskpart(contents: &str) -> Result<String, String> {
    let script_path = temp_root().join(format!(
        "block-duplicator-diskpart-{}.txt",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock error")
            .as_nanos()
    ));
    std::fs::write(&script_path, contents)
        .map_err(|e| format!("failed to write diskpart script: {e}"))?;

    let output = Command::new("diskpart")
        .arg("/s")
        .arg(&script_path)
        .output()
        .map_err(|e| format!("failed to launch diskpart: {e}"))?;
    if !keep_tmp() {
        let _ = std::fs::remove_file(&script_path);
    } else {
        eprintln!("Keeping diskpart script: {}", script_path.display());
    }

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn ps_single_quoted(input: &str) -> String {
    input.replace('\'', "''")
}

pub fn keep_tmp() -> bool {
    matches!(std::env::var("BLOCK_DUP_KEEP_TMP").ok().as_deref(), Some("1"))
}

fn temp_root() -> PathBuf {
    let dir = std::env::temp_dir().join("_block-duplicator");
    std::fs::create_dir_all(&dir).expect("create test temp root");
    dir
}
