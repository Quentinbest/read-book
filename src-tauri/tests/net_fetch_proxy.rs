//! `net.fetch` never goes through a proxy named in the environment (`HTTP_PROXY`,
//! `ALL_PROXY`): a request reaches only the host the extension was granted. Its
//! own test binary, since it sets environment variables, which other tests running
//! at the same time would race with.

use linen_lib::extensions::registry::net_fetch;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Answers every connection with `status`, counting them.
fn answer(status: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { continue };
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            while !buf.ends_with(b"\r\n\r\n") && s.read_exact(&mut byte).is_ok() {
                buf.push(byte[0]);
            }
            let _ = write!(
                s,
                "HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            );
        }
    });
    (addr, count)
}

#[test]
fn a_proxy_named_in_the_environment_is_never_used() {
    let (proxy, proxied) = answer("599 Proxy");
    // Set before any request, and before the servers' threads read anything.
    for var in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        std::env::set_var(var, format!("http://{proxy}"));
    }
    std::env::remove_var("NO_PROXY");
    std::env::remove_var("no_proxy");
    let (host, served) = answer("200 OK");
    let granted = vec![format!("network:{host}")];
    let r = net_fetch(
        &granted,
        &format!("http://{host}/x"),
        "GET",
        None,
        &[],
        None,
    )
    .unwrap();
    assert_eq!((r.status, r.body.as_str()), (200, "ok"));
    assert_eq!(served.load(Ordering::SeqCst), 1);
    assert_eq!(proxied.load(Ordering::SeqCst), 0);
}
