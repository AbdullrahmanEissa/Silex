use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

pub async fn wait_for_shutdown(tx: watch::Sender<bool>) {
    if let (Ok(mut sigint), Ok(mut sigterm)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
    ) {
        tokio::select! {
            _ = sigint.recv() => {
                let _ = tx.send(true);
            }
            _ = sigterm.recv() => {
                let _ = tx.send(true);
            }
        }
    }
}