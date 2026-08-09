use crate::core::io::{BlockSink, BlockSource};

pub struct TransferStats {
    pub bytes_copied: u64,
    pub source_len: u64,
}

#[allow(dead_code)]
pub fn copy_all<S, D>(source: &mut S, sink: &mut D, chunk_size: usize) -> std::io::Result<TransferStats>
where
    S: BlockSource + ?Sized,
    D: BlockSink + ?Sized,
{
    copy_all_with_progress(source, sink, chunk_size, |_copied, _total| {})
}

pub fn copy_all_with_progress<S, D, F>(
    source: &mut S,
    sink: &mut D,
    chunk_size: usize,
    mut on_progress: F,
) -> std::io::Result<TransferStats>
where
    S: BlockSource + ?Sized,
    D: BlockSink + ?Sized,
    F: FnMut(u64, u64),
{
    let source_caps = source.capabilities();
    let sink_caps = sink.capabilities();
    let _requires_elevation = source_caps.requires_elevation || sink_caps.requires_elevation;
    if !source_caps.random_access || !sink_caps.random_access {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "non-random-access backends are not implemented yet",
        ));
    }

    let preferred_block = source.block_size().max(sink.block_size()).max(1);
    let chunk_size = chunk_size.max(preferred_block);

    let source_len = source.len()?;
    // Opportunistic pre-flight: fixed-capacity sinks (block devices, attached
    // VHDX disks) report a real length; growable or streaming sinks report 0
    // and are skipped. Prevents partially overwriting a too-small disk.
    let sink_len = sink.len()?;
    if sink_len > 0 && sink_len < source_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "destination is smaller than source ({sink_len} < {source_len} bytes)"
            ),
        ));
    }
    let mut copied = 0_u64;
    let mut buf = vec![0_u8; chunk_size];

    while copied < source_len {
        let remaining = (source_len - copied) as usize;
        let to_read = remaining.min(buf.len());
        let read = source.read_at(copied, &mut buf[..to_read])?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "source ended prematurely: got {copied} of {source_len} bytes"
                ),
            ));
        }

        let mut written_total = 0usize;
        while written_total < read {
            let written = sink.write_at(copied + written_total as u64, &buf[written_total..read])?;
            if written == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "sink returned zero bytes written",
                ));
            }
            written_total += written;
        }

        copied += read as u64;
        on_progress(copied, source_len);
    }

    sink.flush()?;

    Ok(TransferStats {
        bytes_copied: copied,
        source_len,
    })
}
