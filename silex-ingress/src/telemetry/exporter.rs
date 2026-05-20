use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use crate::telemetry::atomics::AtomicMetrics;

pub async fn start_prometheus_exporter(
    addr: &str,
    metrics: Arc<AtomicMetrics>,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;

    loop {
        if let Ok((mut stream, _)) = listener.accept().await {
            let reqs = metrics.get_requests();
            let latency = metrics.get_total_latency_ms();

            let body = format!(
                "# HELP silex_requests_total Total HTTP requests processed\n\
                 # TYPE silex_requests_total counter\n\
                 silex_requests_total {}\n\
                 # HELP silex_latency_ms_total Total latency of all requests in milliseconds\n\
                 # TYPE silex_latency_ms_total counter\n\
                 silex_latency_ms_total {}\n",
                reqs, latency
            );

            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 Content-Type: text/plain; version=0.0.4\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\
                 \r\n\
                 {}",
                body.len(),
                body
            );

            tokio::spawn(async move {
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    }
}