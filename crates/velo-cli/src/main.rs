//! Test harness for the Velo engine and scheduler.
//!
//!   velo get   <url> [out_dir] [segments]   one download, straight to the engine
//!   velo queue <out_dir> <url> <url> ...    many downloads through the scheduler

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use velo_core::types::{DownloadSpec, DownloadStatus};
use velo_core::{build_client, DEFAULT_USER_AGENT};
use velo_manager::{Event, Manager};
use velo_store::models::NewDownload;
use velo_store::Store;

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
    match args.get(1).map(String::as_str) {
        Some("get") => single(&args[2..]).await,
        Some("queue") => queue(&args[2..]).await,
        _ => {
            eprintln!(
                "usage:\n  velo get <url> [out_dir] [segments]\n  velo queue <out_dir> <url>..."
            );
            std::process::exit(2);
        }
    }
}

async fn single(args: &[String]) -> anyhow::Result<()> {
    let Some(url) = args.first().cloned() else {
        anyhow::bail!("missing url");
    };
    let out_dir = args.get(1).cloned().unwrap_or_else(|| "./downloads".into());
    let segments: u8 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);

    let client = build_client(DEFAULT_USER_AGENT)?;
    let spec = DownloadSpec {
        url,
        out_dir,
        file_name: None,
        headers: HashMap::new(),
        segments,
    };

    let started = Instant::now();
    let done = Arc::new(AtomicU64::new(0));
    let flag = done.clone();

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
            flag.store(p.status as u64 + 1, Ordering::SeqCst);
        }
    })
    .await?;

    println!(
        "file: {}\nsize: {}\nranges: {}\n",
        handle.path.display(),
        handle
            .probe
            .total_size
            .map(human)
            .unwrap_or_else(|| "unknown".into()),
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

/// Feeds many urls to the scheduler and prints when each one starts and ends,
/// so we can see that only `max_concurrent` ever run at the same time.
async fn queue(args: &[String]) -> anyhow::Result<()> {
    let Some(out_dir) = args.first().cloned() else {
        anyhow::bail!("missing out_dir");
    };
    let urls = &args[1..];
    anyhow::ensure!(!urls.is_empty(), "no urls given");

    let store = Arc::new(Store::open("./velo-test.db")?);
    let (mgr, mut events) = Manager::new(store)?;
    mgr.start()?;

    let items: Vec<NewDownload> = urls
        .iter()
        .map(|u| NewDownload {
            url: u.clone(),
            out_dir: out_dir.clone(),
            file_name: None,
            headers: HashMap::new(),
            segments: Some(8),
            batch_id: None,
            scheduled_at: None,
            priority: None,
        })
        .collect();

    let (batch_id, ids) = mgr.add_batch("cli test", None, &items)?;
    println!(
        "batch {batch_id}: queued {} downloads, {} at a time\n",
        ids.len(),
        mgr.max_concurrent()
    );

    let total = ids.len();
    let mut finished = 0usize;
    let mut running = 0usize;
    let started = Instant::now();

    while finished < total {
        let Some(ev) = events.recv().await else { break };
        match ev {
            Event::Started {
                id,
                file_name,
                total: size,
            } => {
                running += 1;
                println!(
                    "[{:>5.1}s] START  #{id} {file_name} ({})  running={running}",
                    started.elapsed().as_secs_f64(),
                    size.map(human).unwrap_or_else(|| "?".into())
                );
            }
            Event::Finished { id, path } => {
                running = running.saturating_sub(1);
                finished += 1;
                println!(
                    "[{:>5.1}s] DONE   #{id} {path}  running={running}  {finished}/{total}",
                    started.elapsed().as_secs_f64()
                );
            }
            Event::Failed { id, error } => {
                running = running.saturating_sub(1);
                finished += 1;
                println!(
                    "[{:>5.1}s] FAIL   #{id} {error}  {finished}/{total}",
                    started.elapsed().as_secs_f64()
                );
            }
            _ => {}
        }
    }

    println!(
        "\nall {total} done in {:.1}s",
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
