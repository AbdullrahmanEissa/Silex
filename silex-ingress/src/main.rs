use dashmap::DashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct RoutingTable {
    routes: DashMap<String, String>,
}

impl RoutingTable {
    pub fn new() -> Self {
        Self {
            routes: DashMap::new(),
        }
    }

    pub fn insert(&self, host: String, target_ip: String) {
        self.routes.insert(host, target_ip);
    }

    pub fn lookup(&self, host: &str) -> Option<String> {
        self.routes.get(host).map(|v| v.clone())
    }
}

pub fn parse_http_request(buf: &[u8]) -> Option<(&str, &str)> {
    let s1 = buf.iter().position(|&b| b == b' ')?;
    let s2 = s1 + 1 + buf[s1 + 1..].iter().position(|&b| b == b' ')?;
    let path = std::str::from_utf8(&buf[s1 + 1..s2]).ok()?;

    let mut host = None;
    let mut idx = 0;
    
    while let Some(n) = buf[idx..].iter().position(|&b| b == b'\n') {
        idx += n + 1;
        let line = &buf[idx..];
        
        if line.len() > 6 && (line.starts_with(b"Host: ") || line.starts_with(b"host: ")) {
            let end = line.iter().position(|&b| b == b'\r' || b == b'\n').unwrap_or(line.len());
            if let Ok(h) = std::str::from_utf8(&line[6..end]) {
                host = Some(h.trim());
            }
            break;
        }
    }

    Some((path, host?))
}

async fn proxy_connection(mut client: TcpStream, initial_buf: &[u8], target: &str) {
    let mut server = match TcpStream::connect(target).await {
        Ok(s) => s,
        Err(_) => return,
    };

    if server.write_all(initial_buf).await.is_err() {
        return;
    }

    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
}

async fn start_api_server(table: Arc<RoutingTable>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:9090").await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let t = Arc::clone(&table);
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            if let Ok(n) = stream.read(&mut buf).await {
                let req = String::from_utf8_lossy(&buf[..n]);
                if let (Some(h_idx), Some(i_idx)) = (req.find("\"host\":\""), req.find("\"ip\":\"")) {
                    let h_sub = &req[h_idx + 8..];
                    let i_sub = &req[i_idx + 6..];
                    if let (Some(h_end), Some(i_end)) = (h_sub.find("\""), i_sub.find("\"")) {
                        let host = &h_sub[..h_end];
                        let ip = &i_sub[..i_end];
                        let target = format!("{}:80", ip);
                        t.insert(host.to_string(), target);
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
                    }
                }
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let routing_table = Arc::new(RoutingTable::new());
    
    let api_table = Arc::clone(&routing_table);
    tokio::spawn(async move {
        let _ = start_api_server(api_table).await;
    });

    let listener = TcpListener::bind("0.0.0.0:80").await?;

    loop {
        let (mut stream, _) = listener.accept().await?;
        let table_clone = Arc::clone(&routing_table);

        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            if let Ok(n) = stream.read(&mut buf).await {
                if n > 0 {
                    if let Some((_, host)) = parse_http_request(&buf[..n]) {
                        if let Some(target_ip) = table_clone.lookup(host) {
                            proxy_connection(stream, &buf[..n], &target_ip).await;
                        }
                    }
                }
            }
        });
    }
}