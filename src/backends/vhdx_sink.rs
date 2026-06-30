#[cfg(target_os = "windows")]
use crate::backends::block_sink::BlockDeviceSink;
#[cfg(target_os = "windows")]
use crate::core::io::{BackendCapabilities, BlockSink};
#[cfg(target_os = "windows")]
use std::io;
#[cfg(target_os = "windows")]
use std::process::Command;

#[cfg(target_os = "windows")]
pub struct VhdxSink {
    inner: BlockDeviceSink,
    image_path: String,
}

#[cfg(target_os = "windows")]
impl VhdxSink {
    pub fn create(image_path: &str, size_bytes: u64, block_size: usize) -> io::Result<Self> {
        if size_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "vhdx size must be > 0",
            ));
        }

        let size_mb = ((size_bytes + (1024 * 1024 - 1)) / (1024 * 1024)).max(8);
        let script = format!(
            "create vdisk file=\"{}\" maximum={} type=expandable\nselect vdisk file=\"{}\"\nattach vdisk\n",
            image_path, size_mb, image_path
        );
        run_diskpart(&script)?;

        let disk_number = mounted_disk_number_for_image(image_path)?;
        let sink_path = format!(r"\\.\PhysicalDrive{}", disk_number);
        let inner = BlockDeviceSink::open(&sink_path, block_size)?;

        Ok(Self {
            inner,
            image_path: image_path.to_string(),
        })
    }
}

#[cfg(target_os = "windows")]
impl Drop for VhdxSink {
    fn drop(&mut self) {
        let p = ps_single_quoted(&self.image_path);
        let _ = run_powershell(&format!(
            "Dismount-DiskImage -ImagePath '{p}' -ErrorAction SilentlyContinue"
        ));
    }
}

#[cfg(target_os = "windows")]
impl BlockSink for VhdxSink {
    fn len(&self) -> io::Result<u64> {
        self.inner.len()
    }

    fn block_size(&self) -> usize {
        self.inner.block_size()
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            random_access: true,
            requires_elevation: true,
        }
    }

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<usize> {
        self.inner.write_at(offset, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(target_os = "windows")]
fn mounted_disk_number_for_image(image_path: &str) -> io::Result<u32> {
    let p = ps_single_quoted(image_path);
    let out = run_powershell(&format!(
        "(Get-DiskImage -ImagePath '{p}' -ErrorAction Stop | Get-Disk -ErrorAction Stop).Number"
    ))?;
    out.trim()
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "failed to parse disk number"))
}

#[cfg(target_os = "windows")]
fn run_powershell(script: &str) -> io::Result<String> {
    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(script)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(io::Error::other(String::from_utf8_lossy(&output.stderr)))
    }
}

#[cfg(target_os = "windows")]
fn run_diskpart(contents: &str) -> io::Result<()> {
    let script_path = std::env::temp_dir().join(format!(
        "block-duplicator-vhdx-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock error")
            .as_nanos()
    ));
    std::fs::write(&script_path, contents)?;

    let output = Command::new("diskpart")
        .arg("/s")
        .arg(&script_path)
        .output()?;
    let _ = std::fs::remove_file(&script_path);

    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(String::from_utf8_lossy(&output.stderr)))
    }
}

#[cfg(target_os = "windows")]
fn ps_single_quoted(s: &str) -> String {
    s.replace('\'', "''")
}
