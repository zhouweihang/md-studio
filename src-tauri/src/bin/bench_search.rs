use std::env;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use md_studio_lib::search::{
    SearchEvent, SearchSource, search_notes_blocking, search_stream_blocking,
    warm_snapshot_blocking,
};

/// Streamed requests need a fresh, monotonic id because the backend cancels
/// anything older than the newest one (ADR-008).
static REQUEST: AtomicU64 = AtomicU64::new(1);

struct StreamTiming {
    first_batch: Duration,
    done: Duration,
    hits: usize,
    source: SearchSource,
}

/// Runs one search through the shipped streaming path and records when the
/// first batch reached the `Channel` sink, which is what the UI paints first.
fn stream_once(root: &str, query: &str) -> Result<StreamTiming, String> {
    let request_id = REQUEST.fetch_add(1, Ordering::SeqCst);
    let started = Instant::now();
    let mut first_batch: Option<Duration> = None;
    let mut hits = 0usize;
    let mut source = SearchSource::Disk;

    let mut emit = |event: SearchEvent| {
        match event {
            SearchEvent::Batch { hits: batch } => {
                if first_batch.is_none() {
                    first_batch = Some(started.elapsed());
                }
                hits += batch.len();
            }
            SearchEvent::Done {
                total, source: done_source, ..
            } => {
                hits = total as usize;
                source = done_source;
            }
        }
        true
    };

    search_stream_blocking(root, query, request_id, &mut emit)?;

    let done = started.elapsed();
    Ok(StreamTiming {
        first_batch: first_batch.unwrap_or(done),
        done,
        hits,
        source,
    })
}

struct Args {
    root: String,
    query: String,
    repeat: usize,
    warmup: usize,
    json: bool,
    snapshot: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut root: Option<String> = None;
    let mut query = "needle".to_string();
    let mut repeat = 30usize;
    let mut warmup = 5usize;
    let mut json = false;
    let mut snapshot = true;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = Some(args.next().ok_or_else(|| "--root needs a value".to_string())?)
            }
            "--query" => {
                query = args
                    .next()
                    .ok_or_else(|| "--query needs a value".to_string())?
            }
            "--repeat" => {
                repeat = args
                    .next()
                    .ok_or_else(|| "--repeat needs a value".to_string())?
                    .parse()
                    .map_err(|error| format!("invalid --repeat: {error}"))?
            }
            "--warmup" => {
                warmup = args
                    .next()
                    .ok_or_else(|| "--warmup needs a value".to_string())?
                    .parse()
                    .map_err(|error| format!("invalid --warmup: {error}"))?
            }
            "--json" => json = true,
            "--no-snapshot" => snapshot = false,
            "--help" | "-h" => {
                println!(
                    "usage: bench_search --root <dir> [--query needle] [--repeat 30] \
                     [--warmup 5] [--no-snapshot] [--json]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let root = root.ok_or_else(|| "--root is required".to_string())?;
    if repeat == 0 {
        return Err("--repeat must be at least 1".to_string());
    }

    Ok(Args {
        root,
        query,
        repeat,
        warmup,
        json,
        snapshot,
    })
}

fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = ((fraction * sorted.len() as f64).ceil() as usize)
        .max(1)
        .min(sorted.len());
    sorted[rank - 1]
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn run() -> Result<(), String> {
    let args = parse_args()?;

    let cold_start = Instant::now();
    let cold_hits = search_notes_blocking(&args.root, &args.query)?;
    let cold = cold_start.elapsed();

    // Time the background warm-up the app performs after opening a workspace,
    // then measure queries against the resulting snapshot (RFC-006 §4.4).
    let mut warm_up = 0.0;
    let mut cache = serde_json::Value::Null;
    if args.snapshot {
        let started = Instant::now();
        let status = warm_snapshot_blocking(&args.root)?;
        warm_up = ms(started.elapsed());
        cache = serde_json::json!({
            "state": status.state,
            "total": status.total,
            "cached": status.cached,
            "bytes": status.bytes,
            "budgetBytes": status.budget_bytes,
            "degradedReason": status.degraded_reason
        });
    }

    for _ in 0..args.warmup {
        stream_once(&args.root, &args.query)?;
    }

    let mut samples = Vec::with_capacity(args.repeat);
    let mut first_samples = Vec::with_capacity(args.repeat);
    let mut hits = cold_hits.len();
    let mut reported_source = SearchSource::Disk;
    for _ in 0..args.repeat {
        let timing = stream_once(&args.root, &args.query)?;
        hits = timing.hits;
        reported_source = timing.source;
        samples.push(timing.done);
        first_samples.push(timing.first_batch);
    }
    samples.sort();
    first_samples.sort();

    let p50 = percentile(&samples, 0.50);
    let p95 = percentile(&samples, 0.95);
    let min = samples.first().copied().unwrap_or_default();
    let max = samples.last().copied().unwrap_or_default();
    let first_p50 = percentile(&first_samples, 0.50);
    let first_p95 = percentile(&first_samples, 0.95);

    if args.json {
        let payload = serde_json::json!({
            "root": args.root,
            "query": args.query,
            "hits": hits,
            "runs": args.repeat,
            "warmup": args.warmup,
            "coldMs": ms(cold),
            "warmP50Ms": ms(p50),
            "warmP95Ms": ms(p95),
            "warmMinMs": ms(min),
            "warmMaxMs": ms(max),
            "firstBatchP50Ms": ms(first_p50),
            "firstBatchP95Ms": ms(first_p95),
            "warmUpMs": warm_up,
            "source": match reported_source {
                SearchSource::Snapshot => "snapshot",
                SearchSource::Disk => "disk",
                SearchSource::Mixed => "mixed",
            },
            "cache": cache,
            "peakRssMb": serde_json::Value::Null
        });
        println!(
            "{}",
            serde_json::to_string(&payload).map_err(|error| error.to_string())?
        );
    } else {
        println!("root:   {}", args.root);
        println!("query:  {}", args.query);
        println!("hits:   {hits}");
        println!("cold:   {:.2} ms", ms(cold));
        if args.snapshot {
            println!("warmup: {warm_up:.2} ms (snapshot fill)");
            println!("cache:  {cache}");
        }
        println!("warm:   p50 {:.2} ms / p95 {:.2} ms", ms(p50), ms(p95));
        println!(
            "first:  batch p50 {:.2} ms / p95 {:.2} ms",
            ms(first_p50),
            ms(first_p95)
        );
        println!(
            "range:  {:.2} – {:.2} ms over {} runs ({} warmup)",
            ms(min),
            ms(max),
            args.repeat,
            args.warmup
        );
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("bench_search: {error}");
            ExitCode::FAILURE
        }
    }
}
