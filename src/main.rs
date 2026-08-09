mod backends;
mod core;
#[cfg(target_os = "windows")]
mod elevated_worker;
mod engine;

use backends::file_sink::FileSink;
use backends::file_source::FileSource;
use backends::network_sink::NetworkSink;
use backends::network_source::NetworkSource;
#[cfg(target_os = "windows")]
use backends::vhdx_sink::VhdxSink;
use core::io::{BlockSink, BlockSource};
use engine::transfer::copy_all_with_progress;
use std::io::Write;
use std::time::{Duration, Instant};
#[cfg(any(windows, unix))]
use backends::block_sink::BlockDeviceSink;
#[cfg(any(windows, unix))]
use backends::block_source::BlockDeviceSource;

fn main() {
    println!("-------------------------------------");
    println!(
        "Block Duplicator V{} By DocterDum",
        env!("CARGO_PKG_VERSION")
    );
    println!("-------------------------------------");

    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", usage());
        return;
    }
    let args = match parse_args(raw_args.iter().cloned()) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            print_usage();
            std::process::exit(2);
        }
    };

    #[cfg(target_os = "windows")]
    if args.worker_mode {
        let port = args
            .worker_port
            .expect("--bd-port is required with --bd-worker");
        let token = args
            .worker_token
            .expect("--bd-token is required with --bd-worker");
        if let Err(err) = elevated_worker::run_worker(port, token) {
            eprintln!("Elevated worker failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    let resolved_src_kind = args
        .src_kind
        .unwrap_or_else(|| infer_backend_kind_from_path(&args.src));
    let resolved_dst_kind = args
        .dst_kind
        .unwrap_or_else(|| infer_backend_kind_from_path(&args.dst));

    #[cfg(target_os = "windows")]
    let operation_requests_elevation = args.require_elevation
        || matches!(resolved_src_kind, BackendKind::Block)
        || matches!(resolved_dst_kind, BackendKind::Block | BackendKind::Vhdx);

    #[cfg(target_os = "windows")]
    if operation_requests_elevation && !is_elevated::is_elevated() {
        let exe = match std::env::current_exe() {
            Ok(v) => v,
            Err(err) => {
                eprintln!("Failed to locate current executable for worker launch: {err}");
                std::process::exit(1);
            }
        };
        let (mut source, mut sink) = match elevated_worker::start_elevated_worker_session(
            &exe,
            elevated_worker::backend_kind_to_wire(resolved_src_kind),
            &args.src,
            elevated_worker::backend_kind_to_wire(resolved_dst_kind),
            &args.dst,
            args.chunk_size,
            args.vhdx_size_bytes,
        ) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("Failed to start elevated worker: {err}");
                std::process::exit(1);
            }
        };
        run_copy(&mut *source, &mut *sink, args.chunk_size);
        return;
    }

    #[cfg(unix)]
    {
        let requires_elevation = args.require_elevation
            || matches!(resolved_src_kind, BackendKind::Block)
            || matches!(resolved_dst_kind, BackendKind::Block);
        if requires_elevation && unsafe { libc::geteuid() } != 0 {
            eprintln!("Raw block-device access requires root privileges. Re-run with sudo.");
            std::process::exit(1);
        }
    }

    let mut source: Box<dyn BlockSource> = match resolved_src_kind {
        BackendKind::File => {
            let source = match FileSource::open(&args.src, args.chunk_size) {
                Ok(source) => source,
                Err(err) => {
                    eprintln!("Failed to open source '{}': {err}", args.src);
                    std::process::exit(1);
                }
            };
            Box::new(source) as Box<dyn BlockSource>
        }
        BackendKind::Block => {
            #[cfg(any(windows, unix))]
            {
                let source = match BlockDeviceSource::open(&args.src, args.chunk_size) {
                    Ok(source) => source,
                    Err(err) => {
                        eprintln!("Failed to open block source '{}': {err}", args.src);
                        std::process::exit(1);
                    }
                };
                Box::new(source) as Box<dyn BlockSource>
            }
            #[cfg(not(any(windows, unix)))]
            {
                eprintln!("Block source backend is not supported on this platform.");
                std::process::exit(1);
            }
        }
        BackendKind::Vhdx => {
            eprintln!("VHDX is only supported as a destination backend.");
            std::process::exit(1);
        }
        BackendKind::Network => {
            let source = match NetworkSource::connect(&args.src, args.chunk_size) {
                Ok(v) => v,
                Err(err) => {
                    eprintln!("Failed to open network source '{}': {err}", args.src);
                    std::process::exit(1);
                }
            };
            Box::new(source) as Box<dyn BlockSource>
        }
    };

    let inferred_source_len = source.len().ok();
    let mut sink: Box<dyn BlockSink> = match resolved_dst_kind {
        BackendKind::File => match FileSink::create(&args.dst, args.chunk_size) {
            Ok(sink) => Box::new(sink) as Box<dyn BlockSink>,
            Err(err) => {
                eprintln!("Failed to open destination '{}': {err}", args.dst);
                std::process::exit(1);
            }
        },
        BackendKind::Block => {
            #[cfg(any(windows, unix))]
            {
                let sink = match BlockDeviceSink::open(&args.dst, args.chunk_size) {
                    Ok(sink) => sink,
                    Err(err) => {
                        eprintln!("Failed to open block destination '{}': {err}", args.dst);
                        std::process::exit(1);
                    }
                };
                Box::new(sink) as Box<dyn BlockSink>
            }
            #[cfg(not(any(windows, unix)))]
            {
                eprintln!("Block sink backend is not supported on this platform.");
                std::process::exit(1);
            }
        }
        BackendKind::Vhdx => {
            #[cfg(target_os = "windows")]
            {
                let size = match args.vhdx_size_bytes.or(inferred_source_len) {
                    Some(v) if v > 0 => v,
                    _ => {
                        eprintln!(
                            "VHDX sink requires non-zero size; provide --vhdx-size-bytes or a measurable source length."
                        );
                        std::process::exit(1);
                    }
                };
                match VhdxSink::create(&args.dst, size, args.chunk_size) {
                    Ok(sink) => Box::new(sink) as Box<dyn BlockSink>,
                    Err(err) => {
                        eprintln!("Failed to create VHDX destination '{}': {err}", args.dst);
                        std::process::exit(1);
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                eprintln!("VHDX sink backend is only supported on Windows.");
                std::process::exit(1);
            }
        }
        BackendKind::Network => match NetworkSink::connect(&args.dst, args.chunk_size) {
            Ok(v) => Box::new(v) as Box<dyn BlockSink>,
            Err(err) => {
                eprintln!("Failed to open network destination '{}': {err}", args.dst);
                std::process::exit(1);
            }
        },
    };

    run_copy(&mut *source, &mut *sink, args.chunk_size);
}

struct CliArgs {
    src: String,
    dst: String,
    chunk_size: usize,
    src_kind: Option<BackendKind>,
    dst_kind: Option<BackendKind>,
    vhdx_size_bytes: Option<u64>,
    require_elevation: bool,
    worker_mode: bool,
    worker_port: Option<u16>,
    worker_token: Option<u64>,
}

#[derive(Clone, Copy)]
enum BackendKind {
    File,
    Block,
    Vhdx,
    Network,
}

fn parse_args<I>(mut it: I) -> Result<CliArgs, String>
where
    I: Iterator<Item = String>,
{
    let _program = it.next();

    let mut src: Option<String> = None;
    let mut dst: Option<String> = None;
    let mut chunk_size: usize = 1024 * 1024;
    let mut src_kind: Option<BackendKind> = None;
    let mut dst_kind: Option<BackendKind> = None;
    let mut vhdx_size_bytes: Option<u64> = None;
    let mut require_elevation = false;
    let mut worker_mode = false;
    let mut worker_port: Option<u16> = None;
    let mut worker_token: Option<u64> = None;

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--src" => src = it.next(),
            "--dst" => dst = it.next(),
            "--src-kind" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "Missing value for --src-kind".to_string())?;
                src_kind = Some(parse_backend_kind(&raw)?);
            }
            "--dst-kind" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "Missing value for --dst-kind".to_string())?;
                dst_kind = Some(parse_backend_kind(&raw)?);
            }
            "--chunk-size" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "Missing value for --chunk-size".to_string())?;
                chunk_size = raw
                    .parse::<usize>()
                    .map_err(|_| format!("Invalid --chunk-size value: {raw}"))?;
                if chunk_size == 0 {
                    return Err("--chunk-size must be greater than 0".to_string());
                }
            }
            "--vhdx-size-bytes" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "Missing value for --vhdx-size-bytes".to_string())?;
                let parsed = raw
                    .parse::<u64>()
                    .map_err(|_| format!("Invalid --vhdx-size-bytes value: {raw}"))?;
                if parsed == 0 {
                    return Err("--vhdx-size-bytes must be greater than 0".to_string());
                }
                vhdx_size_bytes = Some(parsed);
            }
            "--require-elevation" => {
                require_elevation = true;
            }
            "--bd-worker" => {
                worker_mode = true;
            }
            "--bd-port" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "Missing value for --bd-port".to_string())?;
                worker_port = Some(
                    raw.parse::<u16>()
                        .map_err(|_| format!("Invalid --bd-port value: {raw}"))?,
                );
            }
            "--bd-token" => {
                let raw = it
                    .next()
                    .ok_or_else(|| "Missing value for --bd-token".to_string())?;
                worker_token = Some(
                    raw.parse::<u64>()
                        .map_err(|_| format!("Invalid --bd-token value: {raw}"))?,
                );
            }
            _ => return Err(format!("Unknown argument: {arg}")),
        }
    }

    let src = src.ok_or_else(|| "Missing required --src <path>".to_string())?;
    let dst = dst.ok_or_else(|| "Missing required --dst <path>".to_string())?;

    Ok(CliArgs {
        src,
        dst,
        chunk_size,
        src_kind,
        dst_kind,
        vhdx_size_bytes,
        require_elevation,
        worker_mode,
        worker_port,
        worker_token,
    })
}

fn usage() -> &'static str {
    "Usage: block-duplicator --src <path> --dst <path> [--src-kind file|block|network] [--dst-kind file|block|vhdx|network] [--chunk-size <bytes>] [--vhdx-size-bytes <bytes>]\nIf kind is omitted, prefix \\\\.\\ implies block; tcp:// implies network; .vhdx implies vhdx; otherwise file."
}

fn print_usage() {
    eprintln!("{}", usage());
}

fn run_copy(source: &mut dyn BlockSource, sink: &mut dyn BlockSink, chunk_size: usize) {
    let mut last_draw = Instant::now()
        .checked_sub(Duration::from_millis(500))
        .unwrap_or_else(Instant::now);
    match copy_all_with_progress(source, sink, chunk_size, |copied, total| {
        if total == 0 {
            return;
        }
        let now = Instant::now();
        if now.duration_since(last_draw) < Duration::from_millis(125) && copied < total {
            return;
        }
        last_draw = now;

        let percent = (copied as f64 / total as f64) * 100.0;
        let _ = std::io::stdout().write_all(
            format!("\rProgress: {:>6.2}% ({}/{})", percent, copied, total).as_bytes(),
        );
        let _ = std::io::stdout().flush();
    }) {
        Ok(stats) => println!(
            "\nCopy complete: {} / {} bytes copied",
            stats.bytes_copied, stats.source_len
        ),
        Err(err) => {
            eprintln!("Copy failed: {err}");
            std::process::exit(1);
        }
    }
}

fn parse_backend_kind(raw: &str) -> Result<BackendKind, String> {
    match raw {
        "file" => Ok(BackendKind::File),
        "block" => Ok(BackendKind::Block),
        "vhdx" => Ok(BackendKind::Vhdx),
        "network" => Ok(BackendKind::Network),
        _ => Err(format!(
            "Invalid backend kind '{raw}'. Expected: file|block|vhdx|network"
        )),
    }
}

fn infer_backend_kind_from_path(path: &str) -> BackendKind {
    if path.starts_with(r"\\.\") {
        BackendKind::Block
    } else if cfg!(unix) && path.starts_with("/dev/") {
        BackendKind::Block
    } else if path.to_ascii_lowercase().starts_with("tcp://") {
        BackendKind::Network
    } else if path.to_ascii_lowercase().ends_with(".vhdx") {
        BackendKind::Vhdx
    } else {
        BackendKind::File
    }
}
