use std::env;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use md_studio_lib::search::search_notes_blocking;

struct Args {
    root: String,
    query: String,
    repeat: usize,
    warmup: usize,
    json: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut root: Option<String> = None;
    let mut query = "needle".to_string();
    let mut repeat = 30usize;
    let mut warmup = 5usize;
    let mut json = false;

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
            "--help" | "-h" => {
                println!(
                    "usage: bench_search --root <dir> [--query needle] [--repeat 30] [--warmup 5] [--json]"
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

    for _ in 0..args.warmup {
        search_notes_blocking(&args.root, &args.query)?;
    }

    let mut samples = Vec::with_capacity(args.repeat);
    let mut hits = cold_hits.len();
    for _ in 0..args.repeat {
        let started = Instant::now();
        hits = search_notes_blocking(&args.root, &args.query)?.len();
        samples.push(started.elapsed());
    }
    samples.sort();

    let p50 = percentile(&samples, 0.50);
    let p95 = percentile(&samples, 0.95);
    let min = samples.first().copied().unwrap_or_default();
    let max = samples.last().copied().unwrap_or_default();

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
        println!("warm:   p50 {:.2} ms / p95 {:.2} ms", ms(p50), ms(p95));
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
