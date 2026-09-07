//! velo <url> [out_dir] [segments]
//! Minimal harness so we can measure the engine before any UI exists.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use velo_core::types::{DownloadSpec, DownloadStatus};
use velo_core::{build_client, DEFAULT_USER_AGENT};

fn human(bytes: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", U[i])
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let args: Vec<String> = std::env::args().collect();
    let Some(url) = args.get(1).cloned() else {
        eprintln!("usage: velo <url> [out_dir] [segments]");
        std::process::exit(2);
    };
    let out_dir = args.get(2).cloned().unwrap_or_else(|| "./downloads".into());
    let segments: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8);

    let client = build_client(DEFAULT_USER_AGENT)?;
    let spec = DownloadSpec { url, out_dir, file_name: None, headers: HashMap::new(), segments };

    let started = Instant::now();
    let done = Arc::new(AtomicU64::new(0));
    let done_flag = done.clone();

    let handle = velo_core::engine::download(&client, spec, None, move |p| {
        let pct = match p.total {
            Some(t) if t > 0 => format!("{:.1}%", p.downloaded as f64 / t as f64 * 100.0),
            _ => "?".into(),
        };
        print!(
            "\r{pct}  {} / {}  {}/s  segs:{}   ",
            human(p.downloaded),
            p.total.map(human).unwrap_or_else(|| "?".into()),
            human(p.bytes_per_sec),
            p.active_segments
        );
        use std::io::Write;
        let _ = std::io::stdout().flush();

        if matches!(p.status, DownloadStatus::Completed | DownloadStatus::Failed) {
            done_flag.store(p.status as u64 + 1, Ordering::SeqCst);
        }
    })
    .await?;

    println!(
        "file: {}\nsize: {}\nranges: {}\n",
        handle.path.display(),
        handle.probe.total_size.map(human).unwrap_or_else(|| "unknown".into()),
        handle.probe.supports_range
    );

    while done.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    let secs = started.elapsed().as_secs_f64();
    let snap = handle.snapshot();
    println!(
        "\n\ndone in {secs:.1}s  avg {}/s  status {:?}",
        human((snap.downloaded as f64 / secs.max(0.001)) as u64),
        snap.status
    );
    Ok(())
}
