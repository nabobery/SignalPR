use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;

/// Configuration for WebSocket connection behavior.
#[allow(dead_code)]
pub struct WsConfig {
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub backoff_multiplier: f64,
}

impl Default for WsConfig {
    fn default() -> Self {
        Self {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            backoff_multiplier: 2.0,
        }
    }
}

/// Compute the next backoff duration with exponential growth, cap, and jitter.
pub fn next_backoff(current: Duration, config: &WsConfig) -> Duration {
    let next_ms = (current.as_millis() as f64 * config.backoff_multiplier) as u128;
    let capped_ms = next_ms.min(config.max_backoff.as_millis());
    // Add 0-25% jitter
    let jitter_factor = 1.0 + (pseudo_random_fraction() * 0.25);
    let with_jitter = (capped_ms as f64 * jitter_factor) as u64;
    let final_ms = (with_jitter as u128).min(config.max_backoff.as_millis()) as u64;
    Duration::from_millis(final_ms)
}

/// Simple pseudo-random fraction [0, 1) using current time nanos.
/// Not cryptographic -- fine for jitter.
fn pseudo_random_fraction() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 1000) as f64 / 1000.0
}

/// Type alias for the async URL-provider closure.
pub type UrlProvider =
    Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result<Url, String>> + Send>> + Send + Sync>;

/// Run a managed WebSocket loop with automatic reconnection.
///
/// - `url_provider`: async closure that returns the WebSocket URL (called on each connect/reconnect)
/// - `config`: backoff configuration
/// - `incoming_tx`: channel to forward incoming text messages
/// - `outgoing_rx`: channel to receive outgoing text messages to send
/// - `status_callback`: called with `true` on connect, `false` on disconnect
/// - `cancel`: token to trigger clean shutdown
#[allow(dead_code)]
pub async fn run_ws_loop(
    url_provider: UrlProvider,
    config: WsConfig,
    incoming_tx: mpsc::Sender<String>,
    mut outgoing_rx: mpsc::Receiver<String>,
    status_callback: Box<dyn Fn(bool) + Send + Sync>,
    cancel: CancellationToken,
) {
    let mut backoff = config.initial_backoff;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        // 1. Get URL
        let url = tokio::select! {
            _ = cancel.cancelled() => break,
            result = (url_provider)() => {
                match result {
                    Ok(u) => u,
                    Err(e) => {
                        tracing::warn!("Failed to obtain WebSocket URL: {}", e);
                        // Wait with backoff before retrying
                        tokio::select! {
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep(backoff) => {}
                        }
                        backoff = next_backoff(backoff, &config);
                        continue;
                    }
                }
            }
        };

        // 2. Connect
        let ws_stream = tokio::select! {
            _ = cancel.cancelled() => break,
            result = connect_async(url.as_str()) => {
                match result {
                    Ok((stream, _response)) => stream,
                    Err(e) => {
                        tracing::warn!("WebSocket connect failed: {}", e);
                        tokio::select! {
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep(backoff) => {}
                        }
                        backoff = next_backoff(backoff, &config);
                        continue;
                    }
                }
            }
        };

        // Connected successfully -- reset backoff
        backoff = config.initial_backoff;
        status_callback(true);
        tracing::debug!("WebSocket connected");

        // 3. Split and run read/write
        let (mut write, mut read) = ws_stream.split();

        let session_cancel = CancellationToken::new();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    session_cancel.cancel();
                    break;
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if incoming_tx.send(text.to_string()).await.is_err() {
                                tracing::warn!("Incoming message receiver dropped");
                                session_cancel.cancel();
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            tracing::debug!("WebSocket received close frame");
                            session_cancel.cancel();
                            break;
                        }
                        Some(Ok(Message::Ping(data))) => {
                            let _ = write.send(Message::Pong(data)).await;
                        }
                        Some(Ok(_)) => {
                            // Ignore binary, pong, etc.
                        }
                        Some(Err(e)) => {
                            tracing::warn!("WebSocket read error: {}", e);
                            session_cancel.cancel();
                            break;
                        }
                        None => {
                            tracing::debug!("WebSocket stream ended");
                            session_cancel.cancel();
                            break;
                        }
                    }
                }
                outgoing = outgoing_rx.recv() => {
                    match outgoing {
                        Some(text) => {
                            if let Err(e) = write.send(Message::Text(text.into())).await {
                                tracing::warn!("WebSocket write error: {}", e);
                                session_cancel.cancel();
                                break;
                            }
                        }
                        None => {
                            // Outgoing channel closed
                            tracing::debug!("Outgoing channel closed");
                            session_cancel.cancel();
                            break;
                        }
                    }
                }
            }
        }

        // Disconnected
        status_callback(false);
        tracing::debug!("WebSocket disconnected");

        if cancel.is_cancelled() {
            break;
        }

        // Backoff before reconnect
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = next_backoff(backoff, &config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_calculation() {
        let config = WsConfig {
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            backoff_multiplier: 2.0,
        };

        // 1s -> ~2s (with up to 25% jitter)
        let b1 = next_backoff(Duration::from_secs(1), &config);
        assert!(b1.as_millis() >= 2000);
        assert!(b1.as_millis() <= 2500);

        // 2s -> ~4s
        let b2 = next_backoff(Duration::from_secs(2), &config);
        assert!(b2.as_millis() >= 4000);
        assert!(b2.as_millis() <= 5000);

        // 32s -> capped at 60s
        let b_big = next_backoff(Duration::from_secs(32), &config);
        assert!(b_big.as_millis() <= 60000);

        // Already at max -> stays at max
        let b_max = next_backoff(Duration::from_secs(60), &config);
        assert!(b_max.as_millis() <= 60000);
    }

    #[test]
    fn test_ws_config_defaults() {
        let config = WsConfig::default();
        assert_eq!(config.initial_backoff, Duration::from_secs(1));
        assert_eq!(config.max_backoff, Duration::from_secs(60));
        assert!((config.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }
}
