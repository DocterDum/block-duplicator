#[derive(Debug, Clone, Copy, Default)]
pub struct BackendCapabilities {
    pub random_access: bool,
    pub requires_elevation: bool,
}

pub trait BlockSource {
    fn len(&self) -> std::io::Result<u64>;
    fn block_size(&self) -> usize;
    fn capabilities(&self) -> BackendCapabilities;
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize>;
}

pub trait BlockSink {
    fn len(&self) -> std::io::Result<u64>;
    fn block_size(&self) -> usize;
    fn capabilities(&self) -> BackendCapabilities;
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> std::io::Result<usize>;
    fn flush(&mut self) -> std::io::Result<()>;
}
