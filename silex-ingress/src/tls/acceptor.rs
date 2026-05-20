use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_rustls::TlsAcceptor;
use rustls::ServerConfig;
use crate::tls::cert_store::SilexCertResolver;

pub async fn start_tls_server(
    listener: TcpListener,
    cert_resolver: Arc<SilexCertResolver>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> std::io::Result<()> {
    
    let tls_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_cert_resolver(cert_resolver);

    let acceptor = TlsAcceptor::from(Arc::new(tls_config));

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                if let Ok((tcp_stream, _)) = accept_result {
                    let acceptor_clone = acceptor.clone();
                    tokio::spawn(async move {
                        if let Ok(_tls_stream) = acceptor_clone.accept(tcp_stream).await {
                        }
                    });
                }
            }
            _ = shutdown_rx.changed() => {
                break;
            }
        }
    }

    Ok(())
}