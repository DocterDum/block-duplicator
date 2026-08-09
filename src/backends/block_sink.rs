#[cfg(target_os = "windows")]
use std::fs::OpenOptions;
#[cfg(target_os = "windows")]
use std::io::{self, Seek, SeekFrom, Write};
#[cfg(target_os = "windows")]
use std::mem::size_of;
#[cfg(target_os = "windows")]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(target_os = "windows")]
use std::os::windows::io::AsRawHandle;
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::IO::DeviceIoControl;

#[cfg(target_os = "windows")]
use crate::core::io::{BackendCapabilities, BlockSink};

#[cfg(target_os = "windows")]
const FILE_SHARE_READ: u32 = 0x1;
#[cfg(target_os = "windows")]
const FILE_SHARE_WRITE: u32 = 0x2;
#[cfg(target_os = "windows")]
const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405c;

#[cfg(target_os = "windows")]
#[repr(C)]
struct GetLengthInformation {
    length: i64,
}

#[cfg(target_os = "windows")]
pub struct BlockDeviceSink {
    file: std::fs::File,
    block_size: usize,
}

#[cfg(target_os = "windows")]
impl BlockDeviceSink {
    pub fn open(device_path: &str, block_size: usize) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(device_path)?;
        Ok(Self { file, block_size })
    }
}

#[cfg(target_os = "windows")]
impl BlockSink for BlockDeviceSink {
    fn len(&self) -> std::io::Result<u64> {
        query_device_length(&self.file)
    }

    fn block_size(&self) -> usize {
        self.block_size
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            random_access: true,
            requires_elevation: true,
        }
    }

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> std::io::Result<usize> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        // FlushFileBuffers: force written data through the OS cache to the device.
        self.file.sync_all()
    }
}

#[cfg(target_os = "windows")]
fn query_device_length(file: &std::fs::File) -> io::Result<u64> {
    let mut out = GetLengthInformation { length: 0 };
    let mut returned = 0u32;

    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle() as _,
            IOCTL_DISK_GET_LENGTH_INFO,
            std::ptr::null(),
            0,
            (&mut out as *mut GetLengthInformation).cast(),
            size_of::<GetLengthInformation>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };

    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    if out.length < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "device length is negative",
        ));
    }

    Ok(out.length as u64)
}

#[cfg(unix)]
mod unix {
    use std::fs::OpenOptions;
    use std::io::{Seek, SeekFrom, Write};
    use std::os::unix::io::AsRawFd;

    use crate::backends::block_source::blkgetsize64::device_size;
    use crate::core::io::{BackendCapabilities, BlockSink};

    pub struct BlockDeviceSink {
        file: std::fs::File,
        block_size: usize,
    }

    impl BlockDeviceSink {
        pub fn open(device_path: &str, block_size: usize) -> std::io::Result<Self> {
            let file = OpenOptions::new().read(true).write(true).open(device_path)?;
            Ok(Self { file, block_size })
        }
    }

    impl BlockSink for BlockDeviceSink {
        fn len(&self) -> std::io::Result<u64> {
            device_size(self.file.as_raw_fd())
        }

        fn block_size(&self) -> usize {
            self.block_size
        }

        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                random_access: true,
                requires_elevation: true,
            }
        }

        fn write_at(&mut self, offset: u64, buf: &[u8]) -> std::io::Result<usize> {
            self.file.seek(SeekFrom::Start(offset))?;
            self.file.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.file.flush()?;
            // fsync: force written data through the OS cache to the device.
            self.file.sync_all()
        }
    }
}

#[cfg(unix)]
pub use unix::BlockDeviceSink;
