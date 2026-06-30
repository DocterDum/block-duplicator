#![cfg(target_os = "windows")]

use crate::backends::block_sink::BlockDeviceSink;
use crate::backends::block_source::BlockDeviceSource;
use crate::backends::file_sink::FileSink;
use crate::backends::file_source::FileSource;
use crate::backends::vhdx_sink::VhdxSink;
use crate::core::io::{BlockSink, BlockSource};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const OP_INIT: u8 = 1;
const OP_SOURCE_LEN: u8 = 2;
const OP_READ_AT: u8 = 3;
const OP_WRITE_AT: u8 = 4;
const OP_FLUSH: u8 = 5;
const OP_SHUTDOWN: u8 = 6;

const KIND_FILE: u8 = 1;
const KIND_BLOCK: u8 = 2;
const KIND_VHDX: u8 = 3;

pub fn start_elevated_worker_session(
    exe: &std::path::Path,
    src_kind: u8,
    src_path: &str,
    dst_kind: u8,
    dst_path: &str,
    chunk_size: usize,
    vhdx_size_bytes: Option<u64>,
) -> io::Result<(Box<dyn BlockSource>, Box<dyn BlockSink>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let token = random_token();

    launch_worker(exe, port, token)?;

    let mut stream = accept_with_timeout(&listener, Duration::from_secs(30))?;
    stream.set_nodelay(true)?;

    write_u64(&mut stream, token)?;
    let ack = read_u8(&mut stream)?;
    if ack != 0 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "worker token rejected"));
    }

    write_u8(&mut stream, OP_INIT)?;
    write_u8(&mut stream, src_kind)?;
    write_u8(&mut stream, dst_kind)?;
    write_u32(&mut stream, chunk_size as u32)?;
    write_u64(&mut stream, vhdx_size_bytes.unwrap_or(0))?;
    write_string(&mut stream, src_path)?;
    write_string(&mut stream, dst_path)?;
    expect_ok(&mut stream)?;

    let shared = Rc::new(RefCell::new(stream));
    let source = IpcSource {
        stream: Rc::clone(&shared),
        block_size: chunk_size,
    };
    let sink = IpcSink {
        stream: shared,
        block_size: chunk_size,
    };
    Ok((Box::new(source), Box::new(sink)))
}

pub fn run_worker(port: u16, token: u64) -> io::Result<()> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    let incoming_token = read_u64(&mut stream)?;
    if incoming_token != token {
        write_u8(&mut stream, 1)?;
        return Ok(());
    }
    write_u8(&mut stream, 0)?;

    let mut source: Option<SourceBackend> = None;
    let mut sink: Option<SinkBackend> = None;

    loop {
        let op = match read_u8(&mut stream) {
            Ok(v) => v,
            Err(_) => break,
        };

        match op {
            OP_INIT => {
                let src_kind = read_u8(&mut stream)?;
                let dst_kind = read_u8(&mut stream)?;
                let chunk_size = read_u32(&mut stream)? as usize;
                let vhdx_size_bytes = read_u64(&mut stream)?;
                let src_path = read_string(&mut stream)?;
                let dst_path = read_string(&mut stream)?;

                source = Some(open_source(src_kind, &src_path, chunk_size)?);
                sink = Some(open_sink(
                    dst_kind,
                    &dst_path,
                    chunk_size,
                    if vhdx_size_bytes == 0 {
                        None
                    } else {
                        Some(vhdx_size_bytes)
                    },
                )?);
                write_ok(&mut stream)?;
            }
            OP_SOURCE_LEN => {
                let len = source_mut(&mut source)?.len()?;
                write_ok(&mut stream)?;
                write_u64(&mut stream, len)?;
            }
            OP_READ_AT => {
                let offset = read_u64(&mut stream)?;
                let len = read_u32(&mut stream)? as usize;
                let mut buf = vec![0u8; len];
                let read = source_mut(&mut source)?.read_at(offset, &mut buf)?;
                write_ok(&mut stream)?;
                write_u32(&mut stream, read as u32)?;
                stream.write_all(&buf[..read])?;
            }
            OP_WRITE_AT => {
                let offset = read_u64(&mut stream)?;
                let len = read_u32(&mut stream)? as usize;
                let mut buf = vec![0u8; len];
                stream.read_exact(&mut buf)?;
                let written = sink_mut(&mut sink)?.write_at(offset, &buf)?;
                write_ok(&mut stream)?;
                write_u32(&mut stream, written as u32)?;
            }
            OP_FLUSH => {
                sink_mut(&mut sink)?.flush()?;
                write_ok(&mut stream)?;
            }
            OP_SHUTDOWN => {
                write_ok(&mut stream)?;
                break;
            }
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown op")),
        }
    }

    Ok(())
}

struct IpcSource {
    stream: Rc<RefCell<TcpStream>>,
    block_size: usize,
}

impl BlockSource for IpcSource {
    fn len(&self) -> io::Result<u64> {
        let mut s = self.stream.borrow_mut();
        write_u8(&mut s, OP_SOURCE_LEN)?;
        expect_ok(&mut s)?;
        read_u64(&mut s)
    }
    fn block_size(&self) -> usize {
        self.block_size
    }
    fn capabilities(&self) -> crate::core::io::BackendCapabilities {
        crate::core::io::BackendCapabilities {
            random_access: true,
            requires_elevation: true,
        }
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let mut s = self.stream.borrow_mut();
        write_u8(&mut s, OP_READ_AT)?;
        write_u64(&mut s, offset)?;
        write_u32(&mut s, buf.len() as u32)?;
        expect_ok(&mut s)?;
        let read = read_u32(&mut s)? as usize;
        s.read_exact(&mut buf[..read])?;
        Ok(read)
    }
}

struct IpcSink {
    stream: Rc<RefCell<TcpStream>>,
    block_size: usize,
}

impl BlockSink for IpcSink {
    fn len(&self) -> io::Result<u64> {
        Ok(0)
    }
    fn block_size(&self) -> usize {
        self.block_size
    }
    fn capabilities(&self) -> crate::core::io::BackendCapabilities {
        crate::core::io::BackendCapabilities {
            random_access: true,
            requires_elevation: true,
        }
    }
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<usize> {
        let mut s = self.stream.borrow_mut();
        write_u8(&mut s, OP_WRITE_AT)?;
        write_u64(&mut s, offset)?;
        write_u32(&mut s, buf.len() as u32)?;
        s.write_all(buf)?;
        expect_ok(&mut s)?;
        Ok(read_u32(&mut s)? as usize)
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut s = self.stream.borrow_mut();
        write_u8(&mut s, OP_FLUSH)?;
        expect_ok(&mut s)
    }
}

impl Drop for IpcSink {
    fn drop(&mut self) {
        if let Ok(mut s) = self.stream.try_borrow_mut() {
            let _ = write_u8(&mut s, OP_SHUTDOWN);
            let _ = expect_ok(&mut s);
        }
    }
}

enum SourceBackend {
    File(FileSource),
    Block(BlockDeviceSource),
}
impl BlockSource for SourceBackend {
    fn len(&self) -> io::Result<u64> {
        match self {
            SourceBackend::File(v) => v.len(),
            SourceBackend::Block(v) => v.len(),
        }
    }
    fn block_size(&self) -> usize {
        match self {
            SourceBackend::File(v) => v.block_size(),
            SourceBackend::Block(v) => v.block_size(),
        }
    }
    fn capabilities(&self) -> crate::core::io::BackendCapabilities {
        match self {
            SourceBackend::File(v) => v.capabilities(),
            SourceBackend::Block(v) => v.capabilities(),
        }
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            SourceBackend::File(v) => v.read_at(offset, buf),
            SourceBackend::Block(v) => v.read_at(offset, buf),
        }
    }
}

enum SinkBackend {
    File(FileSink),
    Block(BlockDeviceSink),
    Vhdx(VhdxSink),
}
impl BlockSink for SinkBackend {
    fn len(&self) -> io::Result<u64> {
        match self {
            SinkBackend::File(v) => v.len(),
            SinkBackend::Block(v) => v.len(),
            SinkBackend::Vhdx(v) => v.len(),
        }
    }
    fn block_size(&self) -> usize {
        match self {
            SinkBackend::File(v) => v.block_size(),
            SinkBackend::Block(v) => v.block_size(),
            SinkBackend::Vhdx(v) => v.block_size(),
        }
    }
    fn capabilities(&self) -> crate::core::io::BackendCapabilities {
        match self {
            SinkBackend::File(v) => v.capabilities(),
            SinkBackend::Block(v) => v.capabilities(),
            SinkBackend::Vhdx(v) => v.capabilities(),
        }
    }
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> io::Result<usize> {
        match self {
            SinkBackend::File(v) => v.write_at(offset, buf),
            SinkBackend::Block(v) => v.write_at(offset, buf),
            SinkBackend::Vhdx(v) => v.write_at(offset, buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            SinkBackend::File(v) => v.flush(),
            SinkBackend::Block(v) => v.flush(),
            SinkBackend::Vhdx(v) => v.flush(),
        }
    }
}

fn open_source(kind: u8, path: &str, block: usize) -> io::Result<SourceBackend> {
    match kind {
        KIND_FILE => Ok(SourceBackend::File(FileSource::open(path, block)?)),
        KIND_BLOCK => Ok(SourceBackend::Block(BlockDeviceSource::open(path, block)?)),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid source kind")),
    }
}
fn open_sink(kind: u8, path: &str, block: usize, vhdx_size_bytes: Option<u64>) -> io::Result<SinkBackend> {
    match kind {
        KIND_FILE => Ok(SinkBackend::File(FileSink::create(path, block)?)),
        KIND_BLOCK => Ok(SinkBackend::Block(BlockDeviceSink::open(path, block)?)),
        KIND_VHDX => {
            let size = vhdx_size_bytes.ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "vhdx sink requires size bytes")
            })?;
            Ok(SinkBackend::Vhdx(VhdxSink::create(path, size, block)?))
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid sink kind")),
    }
}

fn source_mut(v: &mut Option<SourceBackend>) -> io::Result<&mut SourceBackend> {
    v.as_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "source not initialized"))
}
fn sink_mut(v: &mut Option<SinkBackend>) -> io::Result<&mut SinkBackend> {
    v.as_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "sink not initialized"))
}

fn accept_with_timeout(listener: &TcpListener, timeout: Duration) -> io::Result<TcpStream> {
    listener.set_nonblocking(true)?;
    let start = Instant::now();
    loop {
        match listener.accept() {
            Ok((s, _)) => return Ok(s),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if start.elapsed() > timeout {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "worker did not connect"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

fn launch_worker(exe: &std::path::Path, port: u16, token: u64) -> io::Result<()> {
    let exe_w = to_wide(exe.to_string_lossy().as_ref());
    let verb = to_wide("runas");
    let params = to_wide(&format!("--bd-worker --bd-port {} --bd-token {}", port, token));
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            exe_w.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if (result as usize) <= 32 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn random_token() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock error")
        .as_nanos() as u64;
    now ^ ((std::process::id() as u64) << 32)
}

fn expect_ok(s: &mut TcpStream) -> io::Result<()> {
    let status = read_u8(s)?;
    if status == 0 {
        return Ok(());
    }
    let msg = read_string(s)?;
    Err(io::Error::other(msg))
}

fn write_ok(s: &mut TcpStream) -> io::Result<()> {
    write_u8(s, 0)
}

fn write_u8(s: &mut TcpStream, v: u8) -> io::Result<()> {
    s.write_all(&[v])
}
fn read_u8(s: &mut TcpStream) -> io::Result<u8> {
    let mut b = [0u8; 1];
    s.read_exact(&mut b)?;
    Ok(b[0])
}
fn write_u32(s: &mut TcpStream, v: u32) -> io::Result<()> {
    s.write_all(&v.to_le_bytes())
}
fn read_u32(s: &mut TcpStream) -> io::Result<u32> {
    let mut b = [0u8; 4];
    s.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn write_u64(s: &mut TcpStream, v: u64) -> io::Result<()> {
    s.write_all(&v.to_le_bytes())
}
fn read_u64(s: &mut TcpStream) -> io::Result<u64> {
    let mut b = [0u8; 8];
    s.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn write_string(s: &mut TcpStream, v: &str) -> io::Result<()> {
    write_u32(s, v.len() as u32)?;
    s.write_all(v.as_bytes())
}
fn read_string(s: &mut TcpStream) -> io::Result<String> {
    let len = read_u32(s)? as usize;
    let mut buf = vec![0u8; len];
    s.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "utf8"))
}

pub fn backend_kind_to_wire(kind: super::BackendKind) -> u8 {
    match kind {
        super::BackendKind::File => KIND_FILE,
        super::BackendKind::Block => KIND_BLOCK,
        super::BackendKind::Vhdx => KIND_VHDX,
        super::BackendKind::Network => {
            panic!("network backend is not supported in elevated worker mode")
        }
    }
}
