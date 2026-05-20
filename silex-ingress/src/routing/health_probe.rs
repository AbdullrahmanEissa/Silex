use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

pub async fn check_backend_health(addr: SocketAddr) -> bool {
    match timeout(Duration::from_millis(50), TcpStream::connect(addr)).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}