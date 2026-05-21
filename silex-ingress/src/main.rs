use base64::{engine::general_purpose, Engine as _};
use dashmap::DashMap;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{Certificate, PrivateKey, ServerConfig};
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

struct Metrics {
    total_req: AtomicU64,
    active_conn: AtomicU64,
    err_5xx: AtomicU64,
}

static METRICS: Metrics = Metrics {
    total_req: AtomicU64::new(0),
    active_conn: AtomicU64::new(0),
    err_5xx: AtomicU64::new(0),
};

#[derive(Deserialize)]
struct RoutePayload {
    host: String,
    ip: String,
}

#[derive(Deserialize)]
struct TlsPayload {
    host: String,
    cert: String,
    key: String,
}

#[derive(Deserialize)]
struct RewritePayload {
    old_path: String,
    new_path: String,
}

struct SniResolver {
    certs: Arc<DashMap<String, Arc<CertifiedKey>>>,
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        if let Some(sni) = client_hello.server_name() {
            if let Some(cert) = self.certs.get(sni) {
                return Some(cert.value().clone());
            }
        }
        None
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let routes = Arc::new(DashMap::new());
    let tls_certs = Arc::new(DashMap::new());
    let rewrites = Arc::new(DashMap::new());
    let cancel_token = CancellationToken::new();
    let (log_tx, mut log_rx) = mpsc::channel::<(String, u16)>(10000);

    tokio::spawn(async move {
        while let Some((host, status)) = log_rx.recv().await {
            println!("ACCESS: host={} status={}", host, status);
        }
    });

    let token_clone = cancel_token.clone();
    tokio::spawn(async move {
        let mut sigint = signal(SignalKind::interrupt()).unwrap();
        let mut sigterm = signal(SignalKind::terminate()).unwrap();
        tokio::select! {
            _ = sigint.recv() => {}
            _ = sigterm.recv() => {}
        }
        token_clone.cancel();
    });

    let routes_api = routes.clone();
    let tls_certs_api = tls_certs.clone();
    let rewrites_api = rewrites.clone();
    let api_token = cancel_token.clone();
    
    tokio::spawn(async move {
        let listener = TcpListener::bind("0.0.0.0:9090").await.unwrap();
        loop {
            tokio::select! {
                _ = api_token.cancelled() => break,
                Ok((mut stream, _)) = listener.accept() => {
                    let routes_clone = routes_api.clone();
                    let tls_clone = tls_certs_api.clone();
                    let rewrites_clone = rewrites_api.clone();
                    tokio::spawn(async move {
                        let mut buf = [0; 4096];
                        if let Ok(n) = stream.read(&mut buf).await {
                            let req = String::from_utf8_lossy(&buf[..n]);
                            if req.starts_with("GET /metrics") {
                                let body = format!(
                                    "silex_requests_total {}\nsilex_connections_active {}\nsilex_errors_5xx {}\n",
                                    METRICS.total_req.load(Ordering::Relaxed),
                                    METRICS.active_conn.load(Ordering::Relaxed),
                                    METRICS.err_5xx.load(Ordering::Relaxed)
                                );
                                let header = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
                                    body.len()
                                );
                                let _ = stream.write_all(header.as_bytes()).await;
                                let _ = stream.write_all(body.as_bytes()).await;
                            } else if let Some(body_start) = req.find("\r\n\r\n") {
                                let body = &req[body_start + 4..];
                                if req.starts_with("POST /tls") {
                                    if let Ok(payload) = serde_json::from_str::<TlsPayload>(body) {
                                        if let (Ok(cert_b), Ok(key_b)) = (general_purpose::STANDARD.decode(&payload.cert), general_purpose::STANDARD.decode(&payload.key)) {
                                            let mut cert_reader = std::io::Cursor::new(cert_b);
                                            if let Ok(certs) = rustls_pemfile::certs(&mut cert_reader) {
                                                let cert_chain = certs.into_iter().map(Certificate).collect();
                                                let mut key_reader = std::io::Cursor::new(&key_b);
                                                let mut keys = rustls_pemfile::pkcs8_private_keys(&mut key_reader).unwrap_or_default();
                                                if keys.is_empty() {
                                                    let mut key_reader = std::io::Cursor::new(&key_b);
                                                    keys = rustls_pemfile::rsa_private_keys(&mut key_reader).unwrap_or_default();
                                                }
                                                if !keys.is_empty() {
                                                    if let Ok(key) = rustls::sign::any_supported_type(&PrivateKey(keys.remove(0))) {
                                                        let certified_key = CertifiedKey::new(cert_chain, key);
                                                        tls_clone.insert(payload.host, Arc::new(certified_key));
                                                    }
                                                }
                                            }
                                        }
                                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK").await;
                                    }
                                } else if req.starts_with("POST /rewrite") {
                                    if let Ok(payload) = serde_json::from_str::<RewritePayload>(body) {
                                        rewrites_clone.insert(payload.old_path.into_bytes(), payload.new_path.into_bytes());
                                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK").await;
                                    }
                                } else if let Ok(payload) = serde_json::from_str::<RoutePayload>(body) {
                                    routes_clone.insert(payload.host, payload.ip);
                                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK").await;
                                }
                            }
                        }
                    });
                }
            }
        }
    });

    let resolver = Arc::new(SniResolver { certs: tls_certs.clone() });
    let mut server_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let tls_acceptor = TlsAcceptor::from(Arc::new(server_config));

    let listener_80 = TcpListener::bind("0.0.0.0:80").await?;
    let listener_443 = TcpListener::bind("0.0.0.0:443").await?;

    let routes_80 = routes.clone();
    let rewrites_80 = rewrites.clone();
    let log_tx_80 = log_tx.clone();
    let token_80 = cancel_token.clone();
    
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = token_80.cancelled() => break,
                Ok((stream, _)) = listener_80.accept() => {
                    METRICS.active_conn.fetch_add(1, Ordering::Relaxed);
                    let peer_ip = stream.peer_addr().map(|a| a.ip().to_string().into_bytes()).unwrap_or_default();
                    let routes_ref = routes_80.clone();
                    let rewrites_ref = rewrites_80.clone();
                    let log_tx_ref = log_tx_80.clone();
                    tokio::spawn(async move {
                        let _ = timeout(
                            Duration::from_secs(30),
                            handle_connection(stream, routes_ref, rewrites_ref, log_tx_ref, peer_ip)
                        ).await;
                        METRICS.active_conn.fetch_sub(1, Ordering::Relaxed);
                    });
                }
            }
        }
    });

    let routes_443 = routes.clone();
    let rewrites_443 = rewrites.clone();
    let log_tx_443 = log_tx.clone();
    let token_443 = cancel_token.clone();
    
    loop {
        tokio::select! {
            _ = token_443.cancelled() => break,
            Ok((stream, _)) = listener_443.accept() => {
                METRICS.active_conn.fetch_add(1, Ordering::Relaxed);
                let peer_ip = stream.peer_addr().map(|a| a.ip().to_string().into_bytes()).unwrap_or_default();
                let routes_ref = routes_443.clone();
                let rewrites_ref = rewrites_443.clone();
                let log_tx_ref = log_tx_443.clone();
                let tls_acc = tls_acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(tls_stream) = tls_acc.accept(stream).await {
                        let _ = timeout(
                            Duration::from_secs(30),
                            handle_connection(tls_stream, routes_ref, rewrites_ref, log_tx_ref, peer_ip)
                        ).await;
                    }
                    METRICS.active_conn.fetch_sub(1, Ordering::Relaxed);
                });
            }
        }
    }

    Ok(())
}

async fn handle_connection<S>(
    mut client_stream: S, 
    routes: Arc<DashMap<String, String>>, 
    rewrites: Arc<DashMap<Vec<u8>, Vec<u8>>>,
    log_tx: mpsc::Sender<(String, u16)>,
    peer_ip: Vec<u8>
) -> std::io::Result<()>
where S: tokio::io::AsyncReadExt + tokio::io::AsyncWriteExt + Unpin
{
    METRICS.total_req.fetch_add(1, Ordering::Relaxed);
    
    let mut buf = [0; 4096];
    let mut n = 0;
    
    while n < buf.len() {
        match client_stream.read(&mut buf[n..]).await {
            Ok(0) => return Ok(()),
            Ok(bytes_read) => {
                n += bytes_read;
                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(e) => return Err(e),
        }
    }

    let mut headers_end = 0;
    for i in 0..n.saturating_sub(3) {
        if buf[i] == b'\r' && buf[i+1] == b'\n' && buf[i+2] == b'\r' && buf[i+3] == b'\n' {
            headers_end = i;
            break;
        }
    }

    let mut path_start = 0;
    let mut path_end = 0;
    for i in 0..headers_end {
        if buf[i] == b' ' {
            if path_start == 0 {
                path_start = i + 1;
            } else if path_end == 0 {
                path_end = i;
                break;
            }
        }
    }

    let mut rewritten_path = None;
    if path_start > 0 && path_end > path_start {
        let path = &buf[path_start..path_end];
        if let Some(entry) = rewrites.get(path) {
            rewritten_path = Some(entry.value().clone());
        }
    }

    let mut target_ip = None;
    let mut host_str = String::new();

    for i in 0..headers_end {
        if buf[i..].starts_with(b"Host: ") {
            let start = i + 6;
            if let Some(end) = buf[start..n].iter().position(|&b| b == b'\r') {
                if let Ok(host) = std::str::from_utf8(&buf[start..start + end]) {
                    host_str = host.to_string();
                    if let Some(entry) = routes.get(host) {
                        target_ip = Some(entry.value().clone());
                    }
                }
            }
            break;
        }
    }

    if let Some(ip) = target_ip {
        if let Ok(mut backend_stream) = TcpStream::connect(ip).await {
            if let Some(new_p) = rewritten_path {
                let _ = backend_stream.write_all(&buf[..path_start]).await;
                let _ = backend_stream.write_all(&new_p).await;
                let _ = backend_stream.write_all(&buf[path_end..headers_end]).await;
            } else {
                let _ = backend_stream.write_all(&buf[..headers_end]).await;
            }

            let _ = backend_stream.write_all(b"\r\nX-Forwarded-For: ").await;
            let _ = backend_stream.write_all(&peer_ip).await;
            let _ = backend_stream.write_all(b"\r\nX-Real-IP: ").await;
            let _ = backend_stream.write_all(&peer_ip).await;
            let _ = backend_stream.write_all(b"\r\n\r\n").await;
            let _ = backend_stream.write_all(&buf[headers_end+4..n]).await;

            let _ = tokio::io::copy_bidirectional(&mut client_stream, &mut backend_stream).await;
            let _ = log_tx.try_send((host_str, 200));
        } else {
            METRICS.err_5xx.fetch_add(1, Ordering::Relaxed);
            let _ = log_tx.try_send((host_str, 502));
            let _ = client_stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n").await;
        }
    } else {
        METRICS.err_5xx.fetch_add(1, Ordering::Relaxed);
        let _ = log_tx.try_send((host_str, 404));
        let _ = client_stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
    }

    Ok(())
}
