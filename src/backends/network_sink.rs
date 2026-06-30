use crate::core::io::{BackendCapabilities, BlockSink};
use std::io::{self, Write};
use std::net::TcpStream;

pub struct NetworkSink {
    stream: TcpStream,
    block_size: usize,
    cursor: u64,
}

impl NetworkSink {
    pub fn connect(endpoint: &str, block_size: usize) -> io::Result<Self> {
        let addr = parse_tcp_endpoint(endpoint)?;
        let stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            block_size,
            cursor: 0,
        })
    }
}

impl BlockSink for NetworkSink {
    fn len(&self) -> io::Result<u64> {
        Ok(0)
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

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<usize> {
        if offset != self.cursor {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "network sink is sequential-only; non-sequential write_at not supported",
            ));
        }
        self.stream.write_all(buf)?;
        self.cursor += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
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
