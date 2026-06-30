use crate::core::io::{BackendCapabilities, BlockSource};
use std::io::{self, Read};
use std::net::TcpStream;

pub struct NetworkSource {
    stream: TcpStream,
    block_size: usize,
    total_len: u64,
    cursor: u64,
}

impl NetworkSource {
    pub fn connect(endpoint: &str, block_size: usize) -> io::Result<Self> {
        let addr = parse_tcp_endpoint(endpoint)?;
        let mut stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;

        // Protocol: first 8 bytes from server are little-endian source length.
        let mut len_buf = [0u8; 8];
        stream.read_exact(&mut len_buf)?;
        let total_len = u64::from_le_bytes(len_buf);

        Ok(Self {
            stream,
            block_size,
            total_len,
            cursor: 0,
        })
    }
}

impl BlockSource for NetworkSource {
    fn len(&self) -> io::Result<u64> {
        Ok(self.total_len)
    }

    fn block_size(&self) -> usize {
        self.block_size
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            // Engine reads in increasing order, so sequential TCP stream is valid.
            random_access: true,
            requires_elevation: false,
        }
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset != self.cursor {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "network source is sequential-only; non-sequential read_at not supported",
            ));
        }
        let read = self.stream.read(buf)?;
        self.cursor += read as u64;
        Ok(read)
    }
}

fn parse_tcp_endpoint(endpoint: &str) -> io::Result<&str> {
    endpoint.strip_prefix("tcp://").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "network endpoint must start with tcp:// (e.g. tcp://127.0.0.1:9000)",
        )
    })
}
