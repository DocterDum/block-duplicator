use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use crate::core::io::{BackendCapabilities, BlockSink};

pub struct FileSink {
    file: File,
    block_size: usize,
}

impl FileSink {
    pub fn create(path: impl AsRef<Path>, block_size: usize) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(path)?;
        Ok(Self { file, block_size })
    }
}

impl BlockSink for FileSink {
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

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> std::io::Result<usize> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()?;
        // Force data to stable storage before the copy is reported as complete.
        self.file.sync_all()
    }
}
