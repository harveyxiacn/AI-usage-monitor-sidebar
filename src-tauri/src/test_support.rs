//! Tiny helpers shared by the backend unit tests. [BACKEND]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, empty directory under the system temp dir. The caller removes it.
pub fn tempdir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let dir = std::env::temp_dir().join(format!(
        "ai-usage-sidebar-test-{}-{}-{}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Minimal one-thread HTTP/1.1 stub for provider tests: answers each request
/// with the next canned response (the last one repeats) and counts requests.
/// It lives for the test process; the thread ends when the process does.
pub struct StubServer {
    pub base: String,
    hits: std::sync::Arc<AtomicU64>,
}

/// `(status line, extra headers, body)`
pub type StubResponse = (&'static str, Vec<(&'static str, &'static str)>, String);

impl StubServer {
    pub fn start(responses: Vec<StubResponse>) -> Self {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stub server");
        let base = format!("http://{}", listener.local_addr().expect("stub address"));
        let hits = std::sync::Arc::new(AtomicU64::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 4096];
                let mut head = Vec::new();
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let n = counter.fetch_add(1, Ordering::SeqCst) as usize;
                let (status, headers, body) = &responses[n.min(responses.len() - 1)];
                let mut out = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                out.push_str(body);
                let _ = stream.write_all(out.as_bytes());
            }
        });
        Self { base, hits }
    }

    /// Requests answered so far.
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::SeqCst)
    }
}
