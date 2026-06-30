use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::core::io::{BackendCapabilities, BlockSource};

pub struct BlockDevceSource {
    device: device,
    block_size: usize,
}

impl BlockDevceSource {
    pub fn open(path: impl AsRef<Path>, block_size: usize) -> std::io::Result<Self> {
        let device = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path.as_ref())?
            .unwrap();
        Ok(Self { device, block_size })
    }
}

impl BlockSource for BlockDevceSource {
    fn len(&self) -> std::io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    fn block_size(&self) -> usize {
        self.block_size
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            random_access: true,
            requires_elevation: false,
        }
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read(buf)
    }
}
