//! Signaling WebSocket client (WSS in production, §7.1).
//!
//! Outgoing envelopes are sent through a sink handle; incoming frames are
//! parsed, validated, and forwarded to an unbounded channel owned by the
//! engine task.

use crate::error::EngineError;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use http::Request;
use remote_protocol::Envelope;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct SignalingClient {
    write: SplitSink<WsStream, Message>,
}

impl SignalingClient {
    /// Connects with `Authorization: Bearer <token>`. Returns the client
    /// plus a receiver of every valid incoming envelope.
    pub async fn connect(
        url: &str,
        token: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Envelope>), EngineError> {
        // Build the handshake request manually: `connect_async` only adds
        // the WebSocket headers for `&str`/`Uri` inputs, not custom
        // `Request`s.
        let uri: http::Uri = url
            .parse()
            .map_err(|e| EngineError::Signaling(format!("invalid signaling url {url}: {e}")))?;
        let host = uri.host().unwrap_or("127.0.0.1");
        let port = uri
            .port_u16()
            .unwrap_or(if uri.scheme_str() == Some("wss") {
                443
            } else {
                80
            });
        let authority = format!("{host}:{port}");
        let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();

        let request = Request::get(uri)
            .header("Host", authority)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", key)
            .header("Authorization", format!("Bearer {token}"))
            .body(())
            .map_err(|e| EngineError::Signaling(format!("build request: {e}")))?;

        let (ws, _response) = connect_async(request)
            .await
            .map_err(|e| EngineError::Signaling(format!("connect {url}: {e}")))?;

        let (write, mut read) = ws.split();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(message) = read.next().await {
                match message {
                    Ok(Message::Text(text)) => match Envelope::from_bytes(text.as_bytes()) {
                        Ok(envelope) => {
                            if tx.send(envelope).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error, "ignoring invalid signaling envelope");
                        }
                    },
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error, "signaling stream error");
                        break;
                    }
                }
            }
            tracing::debug!("signaling reader finished");
        });

        Ok((Self { write }, rx))
    }

    pub async fn send(&mut self, envelope: &Envelope) -> Result<(), EngineError> {
        self.write
            .send(Message::Text(envelope.to_json().into()))
            .await
            .map_err(|e| EngineError::Signaling(format!("send: {e}")))
    }
}
