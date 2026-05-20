pub mod routing;
pub mod sys;
pub mod telemetry;
pub mod tls;

use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use routing::circuit_breaker::CircuitBreaker;
use sys::signals::wait_for_shutdown;
use telemetry::atomics::AtomicMetrics;
use telemetry::exporter::start_prometheus_exporter;
use tls::acceptor::start_tls_server;
use tls::cert_store::SilexCertResolver;
use rustls::sign::CertifiedKey;

fn extract_host(buf: &[u8]) -> Option<&str> {
    let mut idx = 0;
    while idx < buf.len() {
        if let Some(n) = buf[idx..].iter().position(|&b| b == b'\n') {
            let line = &buf[idx..idx + n];
            if line.len() > 5 && line[..5].eq_ignore_ascii_case(b"host:") {
                let mut val_start = 5;
                while val_start < line.len() && (line[val_start] == b' ' || line[val_start] == b'\t') {
                    val_start += 1;
                }
                let mut val_end = line.len();
                if val_end > val_start && line[val_end - 1] == b'\r' {
                    val_end -= 1;
                }
                if let Ok(host_str) = std::str::from_utf8(&line[val_start..val_end]) {
                    return Some(host_str.split(':').next().unwrap_or(host_str));
                }
            }
            idx += n + 1;
        } else {
            break;
        }
    }
    None
}

fn extract_json_value<'a>(buf: &'a [u8], key: &[u8]) -> Option<&'a str> {
    let mut pos = 0;
    while pos + key.len() + 3 <= buf.len() {
        if buf[pos] == b'"' && &buf[pos + 1..pos + 1 + key.len()] == key && buf[pos + 1 + key.len()] == b'"' {
            let mut c_pos = pos + key.len() + 2;
            while c_pos < buf.len() && (buf[c_pos] == b' ' || buf[c_pos] == b':') {
                if buf[c_pos] == b':' {
                    let mut v_start = c_pos + 1;
                    while v_start < buf.len() && buf[v_start] == b' ' {
                        v_start += 1;
                    }
                    if v_start < buf.len() && buf[v_start] == b'"' {
                        v_start += 1;
                        if let Some(v_end_off) = buf[v_start..].iter().position(|&b| b == b'"') {
                            return std::str::from_utf8(&buf[v_start..v_start + v_end_off]).ok();
                        }
                    }
                    break;
                }
                c_pos += 1;
            }
        }
        pos += 1;
    }
    None
}

async fn handle_proxy(
    mut client: TcpStream,
    buf: &[u8],
    routes: Arc<DashMap<String, Vec<SocketAddr>>>,
    cb: Arc<CircuitBreaker>,
    metrics: Arc<AtomicMetrics>,
) {
    let start = tokio::time::Instant::now();
    
    let host = match extract_host(buf) {
        Some(h) => h,
        None => return,
    };

    let target_addrs = match routes.get(host) {
        Some(r) => r.clone(),
        None => return,
    };

    let mut selected_target = None;
    for addr in target_addrs.iter() {
        if cb.is_healthy(addr) {
            selected_target = Some(*addr);
            break;
        }
    }

    let target = match selected_target {
        Some(t) => t,
        None => return,
    };

    if let Ok(mut server) = TcpStream::connect(target).await {
        if server.write_all(buf).await.is_ok() {
            let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
            metrics.record_request(start.elapsed().as_millis() as u64);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    
    let routes: Arc<DashMap<String, Vec<SocketAddr>>> = Arc::new(DashMap::new());
    let cb = Arc::new(CircuitBreaker::new());
    let metrics = Arc::new(AtomicMetrics::new());

    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        wait_for_shutdown(shutdown_tx_clone).await;
    });

    let cb_monitor = cb.clone();
    tokio::spawn(async move {
        CircuitBreaker::monitor(cb_monitor).await;
    });

    let metrics_exporter = metrics.clone();
    tokio::spawn(async move {
        let _ = start_prometheus_exporter("0.0.0.0:9091", metrics_exporter).await;
    });

    if let (Ok(cert_bytes), Ok(key_bytes)) = (std::fs::read("cert.der"), std::fs::read("key.der")) {
        if let Ok(key) = rustls::sign::any_supported_type(&rustls::PrivateKey(key_bytes)) {
            let cert = rustls::Certificate(cert_bytes);
            let fallback = Arc::new(CertifiedKey::new(vec![cert], key));
            let resolver = Arc::new(SilexCertResolver::new(fallback));
            
            if let Ok(tls_listener) = TcpListener::bind("0.0.0.0:443").await {
                let rx_tls = shutdown_rx.clone();
                tokio::spawn(async move {
                    let _ = start_tls_server(tls_listener, resolver, rx_tls).await;
                });
            }
        }
    }

    let admin_listener = TcpListener::bind("127.0.0.1:9090").await?;
    let routes_admin = routes.clone();
    let cb_admin = cb.clone();
    let mut admin_rx = shutdown_rx.clone();
    
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok((mut stream, _)) = admin_listener.accept() => {
                    let routes_admin = routes_admin.clone();
                    let cb_admin = cb_admin.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 1024];
                        if let Ok(n) = stream.read(&mut buf).await {
                            if let (Some(host), Some(ip_str)) = (extract_json_value(&buf[..n], b"host"), extract_json_value(&buf[..n], b"ip")) {
                                if let Ok(addr) = ip_str.parse::<SocketAddr>() {
                                    routes_admin.entry(host.to_string()).or_insert_with(Vec::new).push(addr);
                                    cb_admin.register(addr);
                                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK").await;
                                }
                            }
                        }
                    });
                }
                _ = admin_rx.changed() => break,
            }
        }
    });

    let http_listener = TcpListener::bind("0.0.0.0:80").await?;
    let mut main_rx = shutdown_rx.clone();

    loop {
        tokio::select! {
            Ok((stream, _)) = http_listener.accept() => {
                let routes = routes.clone();
                let cb = cb.clone();
                let metrics = metrics.clone();
                
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let mut client = stream;
                    if let Ok(n) = client.read(&mut buf).await {
                        if n > 0 {
                            handle_proxy(client, &buf[..n], routes, cb, metrics).await;
                        }
                    }
                });
            }
            _ = main_rx.changed() => break,
        }
    }

    Ok(())
}