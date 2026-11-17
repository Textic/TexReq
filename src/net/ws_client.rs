use std::collections::HashMap;

use futures_channel::mpsc::UnboundedSender;
use futures_util::{SinkExt as _, StreamExt as _};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::Connector;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};

use crate::models::{RequestItem, StreamDirection, StreamEntry, StreamStatus};
use crate::net::WorkerEvent;
use crate::net::snippets::{build_resolved_headers, build_resolved_url};

#[derive(Debug)]
pub enum WsCommand {
    SendText(String),
    Disconnect,
}

pub async fn run_ws_session(
    req: RequestItem,
    vars: HashMap<String, String>,
    mut cmd_rx: UnboundedReceiver<WsCommand>,
    tx: UnboundedSender<WorkerEvent>,
) {
    let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Connecting));
    let url = build_resolved_url(&req, &vars);

    let mut ws_req = match url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.unbounded_send(WorkerEvent::StreamError(format!(
                "Invalid WebSocket URL '{url}': {e}"
            )));
            let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
            return;
        }
    };

    for (k, v) in build_resolved_headers(&req, &vars) {
        if let (Ok(name), Ok(val)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(&v),
        ) {
            ws_req.headers_mut().append(name, val);
        }
    }

    let tls_result = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(req.network.accept_invalid_certs)
        .build();

    let connect_res = match tls_result {
        Ok(tls) => {
            tokio_tungstenite::connect_async_tls_with_config(
                ws_req,
                None,
                false,
                Some(Connector::NativeTls(tls)),
            )
            .await
        }
        Err(e) => {
            let _ = tx.unbounded_send(WorkerEvent::StreamError(format!(
                "TLS initialization error: {e}"
            )));
            let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
            return;
        }
    };

    let (ws_stream, _) = match connect_res {
        Ok(pair) => pair,
        Err(e) => {
            let _ = tx.unbounded_send(WorkerEvent::StreamError(format!(
                "Failed to connect WebSocket: {e}"
            )));
            let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
            return;
        }
    };

    let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Connected));
    let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
        StreamDirection::System,
        None,
        format!("Connected to {url}"),
    )));

    let (mut sink, mut stream) = ws_stream.split();

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(WsCommand::SendText(msg)) => {
                        if let Err(e) = sink.send(Message::Text(msg.clone().into())).await {
                            let _ = tx.unbounded_send(WorkerEvent::StreamError(format!("Failed to send WS message: {e}")));
                            break;
                        }
                        let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
                            StreamDirection::Outgoing,
                            None,
                            msg,
                        )));
                    }
                    Some(WsCommand::Disconnect) | None => {
                        let _ = sink.close().await;
                        let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
                            StreamDirection::System,
                            None,
                            "Connection closed",
                        )));
                        break;
                    }
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(txt))) => {
                        let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
                            StreamDirection::Incoming,
                            None,
                            txt.to_string(),
                        )));
                    }
                    Some(Ok(Message::Binary(bin))) => {
                        let preview = match String::from_utf8(bin.to_vec()) {
                            Ok(s) => s,
                            Err(e) => format!("<{} binary bytes>", e.as_bytes().len()),
                        };
                        let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
                            StreamDirection::Incoming,
                            Some("binary".to_string()),
                            preview,
                        )));
                    }
                    Some(Ok(Message::Ping(p))) => {
                        let _ = sink.send(Message::Pong(p)).await;
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let reason = frame
                            .map(|f| format!("Connection closed ({}): {}", f.code, f.reason))
                            .unwrap_or_else(|| "Connection closed by server".to_string());
                        let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
                            StreamDirection::System,
                            None,
                            reason,
                        )));
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        let _ = tx.unbounded_send(WorkerEvent::StreamError(format!("WebSocket error: {e}")));
                        break;
                    }
                    None => {
                        let _ = tx.unbounded_send(WorkerEvent::StreamMessage(StreamEntry::now(
                            StreamDirection::System,
                            None,
                            "WebSocket stream ended",
                        )));
                        break;
                    }
                }
            }
        }
    }

    let _ = tx.unbounded_send(WorkerEvent::StreamStatus(StreamStatus::Disconnected));
}
