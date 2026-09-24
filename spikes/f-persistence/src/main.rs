//! Spike F, persistence half (docs/implementation-plan.md §5, Phase 0).
//!
//! `latency <db> <n>`  n progress upserts, one transaction each; prints p50/p95/max
//! `writer <db>`       writes seq 1.. forever; prints each seq after COMMIT returns
//! `verify <db>`       integrity check; prints the highest committed seq
//! `crash <db> <runs>` spawns `writer`, SIGKILLs it at a random point, then verifies
//!                     that every write reported as committed survived

use rusqlite::{params, Connection};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

fn open(path: &str, synchronous: &str) -> Connection {
    let conn = Connection::open(path).expect("open");
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "synchronous", synchronous).unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS positions (
            book_id TEXT PRIMARY KEY, cfi TEXT NOT NULL, fraction REAL NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS writes (seq INTEGER PRIMARY KEY);",
    )
    .unwrap();
    conn
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn upsert(conn: &mut Connection, i: u64) {
    let tx = conn.transaction().unwrap();
    tx.execute(
        "INSERT INTO positions (book_id, cfi, fraction, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(book_id) DO UPDATE SET cfi = excluded.cfi, fraction = excluded.fraction,
         updated_at = excluded.updated_at",
        params![
            "5a1e0000-0000-4000-8000-000000000001",
            format!("epubcfi(/6/{}!/4/2/{}/1:{})", 4 + 2 * (i % 135), 2 * (i % 40), i % 300),
            (i % 1000) as f64 / 1000.0,
            now_ms()
        ],
    )
    .unwrap();
    tx.execute("INSERT OR REPLACE INTO writes (seq) VALUES (?1)", params![i as i64])
        .unwrap();
    tx.commit().unwrap();
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() as f64 - 1.0) * p).round() as usize]
}

fn latency(path: &str, n: u64) {
    for sync in ["NORMAL", "FULL"] {
        let _ = std::fs::remove_file(path);
        let mut conn = open(path, sync);
        let mut ms: Vec<f64> = (0..n)
            .map(|i| {
                let t = Instant::now();
                upsert(&mut conn, i);
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "synchronous={sync} n={n} p50={:.3}ms p95={:.3}ms p99={:.3}ms max={:.3}ms",
            percentile(&ms, 0.5),
            percentile(&ms, 0.95),
            percentile(&ms, 0.99),
            ms[ms.len() - 1]
        );
    }
}

fn writer(path: &str) {
    let mut conn = open(path, "NORMAL");
    let start: u64 = conn
        .query_row("SELECT COALESCE(MAX(seq), 0) FROM writes", [], |r| r.get::<_, i64>(0))
        .unwrap() as u64;
    let mut out = std::io::stdout().lock();
    for i in start + 1.. {
        upsert(&mut conn, i);
        use std::io::Write;
        writeln!(out, "{i}").unwrap();
        out.flush().unwrap();
    }
}

fn verify(path: &str) -> (bool, u64) {
    let conn = Connection::open(path).expect("reopen");
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    let max: i64 = conn
        .query_row("SELECT COALESCE(MAX(seq), 0) FROM writes", [], |r| r.get(0))
        .unwrap();
    (ok == "ok", max as u64)
}

fn crash(path: &str, runs: u32) {
    let exe = std::env::current_exe().unwrap();
    let _ = std::fs::remove_file(path);
    let mut seed = now_ms() as u64 | 1;
    let mut failures = 0;
    for run in 1..=runs {
        // xorshift: kill after 1..1000 reported writes
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let kill_after = 1 + seed % 1000;
        let mut child = Command::new(&exe)
            .args(["writer", path])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut last_reported = 0u64;
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut seen = 0;
        while let Some(Ok(l)) = lines.next() {
            last_reported = l.parse().unwrap();
            seen += 1;
            if seen >= kill_after {
                break;
            }
        }
        child.kill().unwrap(); // SIGKILL on Unix
        child.wait().unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let (ok, max) = verify(path);
        // The writer may have committed a few more writes after the last line we read;
        // only a committed write that is missing is a failure.
        let pass = ok && max >= last_reported;
        if !pass {
            failures += 1;
        }
        println!(
            "run {run:>2}: killed after {seen} reported writes; last reported {last_reported}, \
             db max {max}, integrity {}, {}",
            if ok { "ok" } else { "FAILED" },
            if pass { "pass" } else { "FAIL" }
        );
    }
    println!("crash runs={runs} failures={failures}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("latency") => latency(&args[2], args[3].parse().unwrap()),
        Some("writer") => writer(&args[2]),
        Some("verify") => {
            let (ok, max) = verify(&args[2]);
            println!("integrity={ok} max_seq={max}");
        }
        Some("crash") => crash(&args[2], args[3].parse().unwrap()),
        _ => eprintln!("usage: latency|writer|verify|crash <db> [n]"),
    }
}
