use base64::{engine::general_purpose, Engine as _};
use dashmap::DashMap;
use httparse;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::{Certificate, PrivateKey, ServerConfig};
use serde::Deserialize;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::signal::unix::{signal, SignalKind};
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

fn create_reuseport_listener(port: u16) -> std::io::Result<std::net::TcpListener> {
    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
    let domain = if addr.is_ipv4() { Domain::IPV4 } else { Domain::IPV6 };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.bind(&addr.into())?;
    socket.listen(65535)?;
    Ok(socket.into())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let routes = Arc::new(DashMap::new());
    let tls_certs = Arc::new(DashMap::new());
    let rewrites = Arc::new(DashMap::new());
    let cancel_token = CancellationToken::new();
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);

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
    let _tls_acceptor = TlsAcceptor::from(Arc::new(server_config));

    println!("==================================================");
    println!("🔥 Silex Apex L7 Ingress Controller is ACTIVE.");
    println!("🔥 Running on {} Parallel Cores.", cores);
    println!("🔥 Listening on port 8888 (Kubernetes Bypass Mode).");
    println!("==================================================");

    for _ in 0..cores {
        let std_listener_8888 = create_reuseport_listener(8888)?;
        std_listener_8888.set_nonblocking(true)?;
        let listener_8888 = TcpListener::from_std(std_listener_8888)?;

        let routes_ref = routes.clone();
        let token_clone = cancel_token.clone();
        
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = token_clone.cancelled() => break,
                    Ok((stream, _)) = listener_8888.accept() => {
                        let _ = stream.set_nodelay(true);
                        let routes_local = routes_ref.clone();
                        
                        tokio::spawn(async move {
                            METRICS.active_conn.fetch_add(1, Ordering::Relaxed);
                            let _ = timeout(
                                Duration::from_secs(30),
                                handle_connection(stream, routes_local)
                            ).await;
                            METRICS.active_conn.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                }
            }
        });
    }

    cancel_token.cancelled().await;
    Ok(())
}

async fn handle_connection<S>(
    mut client_stream: S, 
    routes: Arc<DashMap<String, String>>, 
) -> std::io::Result<()>
where S: tokio::io::AsyncReadExt + tokio::io::AsyncWriteExt + Unpin
{
    METRICS.total_req.fetch_add(1, Ordering::Relaxed);
    
    let mut buf = [0; 4096];
    
    let n = match client_stream.read(&mut buf).await {
        Ok(0) => return Ok(()),
        Ok(bytes_read) => bytes_read,
        Err(e) => return Err(e),
    };

    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut req = httparse::Request::new(&mut headers);
    
    let mut target_ip = None;

    if let Ok(httparse::Status::Complete(_)) | Ok(httparse::Status::Partial) = req.parse(&buf[..n]) {
        for header in req.headers.iter() {
            if header.name.eq_ignore_ascii_case("Host") {
                if let Ok(host) = std::str::from_utf8(header.value) {
                    let clean_host = host.split(':').next().unwrap_or(host);
                    if let Some(entry) = routes.get(clean_host) {
                        target_ip = Some(entry.value().clone());
                    }
                }
                break;
            }
        }
    }

    if let Some(ip) = target_ip {
        if let Ok(mut backend_stream) = TcpStream::connect(ip).await {
            let _ = backend_stream.set_nodelay(true);
            let _ = backend_stream.write_all(&buf[..n]).await;
            let _ = tokio::io::copy_bidirectional(&mut client_stream, &mut backend_stream).await;
        } else {
            METRICS.err_5xx.fetch_add(1, Ordering::Relaxed);
            let _ = client_stream.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n").await;
        }
    } else {
        METRICS.err_5xx.fetch_add(1, Ordering::Relaxed);
        let _ = client_stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
    }

    Ok(())
}
