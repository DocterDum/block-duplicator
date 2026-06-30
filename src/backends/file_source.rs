use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::core::io::{BackendCapabilities, BlockSource};

pub struct FileSource {
    file: File,
    block_size: usize,
}

impl FileSource {
    pub fn open(path: impl AsRef<Path>, block_size: usize) -> std::io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self { file, block_size })
    }
}

impl BlockSource for FileSource {
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
