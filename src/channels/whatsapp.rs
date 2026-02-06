//! WhatsApp channel implementation using a Node.js WebSocket bridge.
//!
//! The bridge uses `@whiskeysockets/baileys` to handle the WhatsApp Web protocol.
//! Communication between Rust and Node.js is via WebSocket.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex as TokioMutex;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, error, info, warn};

use crate::bus::events::{InboundMessage, OutboundMessage};
use crate::channels::base::Channel;
use crate::config::schema::WhatsAppConfig;

/// WhatsApp channel that connects to a Node.js bridge via WebSocket.
pub struct WhatsAppChannel {
    config: WhatsAppConfig,
    bus_tx: UnboundedSender<InboundMessage>,
    running: Arc<AtomicBool>,
    /// Sender for outgoing WebSocket messages (set once connected).
    ws_tx: Arc<TokioMutex<Option<UnboundedSender<String>>>>,
}

impl WhatsAppChannel {
    /// Create a new `WhatsAppChannel`.
    pub fn new(config: WhatsAppConfig, bus_tx: UnboundedSender<InboundMessage>) -> Self {
        Self {
            config,
            bus_tx,
            running: Arc::new(AtomicBool::new(false)),
            ws_tx: Arc::new(TokioMutex::new(None)),
        }
    }

    /// Handle a JSON message from the bridge.
    fn _handle_bridge_message(
        data: &Value,
        bus_tx: &UnboundedSender<InboundMessage>,
        allow_from: &[String],
    ) {
        let msg_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match msg_type {
            "message" => {
                let sender = data
                    .get("sender")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let content = data
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Extract phone number from JID (phone@s.whatsapp.net).
                let chat_id = if sender.contains('@') {
                    sender.split('@').next().unwrap_or(sender)
                } else {
                    sender
                };

                // Check allow list.
                if !allow_from.is_empty()
                    && !allow_from.contains(&chat_id.to_string())
                    && !allow_from.contains(&sender.to_string())
                {
                    debug!("WhatsApp: ignoring message from non-allowed sender {}", chat_id);
                    return;
                }

                let content = if content == "[Voice Message]" {
                    "[Voice Message: Transcription not available for WhatsApp yet]"
                } else {
                    content
                };

                let is_group = data
                    .get("isGroup")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let mut msg = InboundMessage::new("whatsapp", chat_id, sender, content);
                if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                    msg.metadata
                        .insert("message_id".to_string(), json!(id));
                }
                if let Some(ts) = data.get("timestamp") {
                    msg.metadata
                        .insert("timestamp".to_string(), ts.clone());
                }
                msg.metadata
                    .insert("is_group".to_string(), json!(is_group));

                let _ = bus_tx.send(msg);
            }
            "status" => {
                let status = data
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                info!("WhatsApp status: {}", status);
            }
            "qr" => {
                info!("Scan QR code in the bridge terminal to connect WhatsApp");
            }
            "error" => {
                let err = data
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                error!("WhatsApp bridge error: {}", err);
            }
            other => {
                debug!("WhatsApp bridge: unknown message type '{}'", other);
            }
        }
    }
}

#[async_trait]
impl Channel for WhatsAppChannel {
    fn name(&self) -> &str {
        "whatsapp"
    }

    async fn start(&mut self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);

        let bridge_url = self.config.bridge_url.clone();
        let bus_tx = self.bus_tx.clone();
        let running = self.running.clone();
        let ws_tx_slot = self.ws_tx.clone();
        let allow_from = self.config.allow_from.clone();

        info!("Connecting to WhatsApp bridge at {}...", bridge_url);

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                match tokio_tungstenite::connect_async(&bridge_url).await {
                    Ok((ws_stream, _)) => {
                        info!("Connected to WhatsApp bridge");
                        let (write, mut read) = ws_stream.split();

                        // Create an mpsc channel to send messages to the WebSocket.
                        let (out_tx, mut out_rx) =
                            tokio::sync::mpsc::unbounded_channel::<String>();

                        // Store the sender so send() can use it.
                        {
                            let mut slot = ws_tx_slot.lock().await;
                            *slot = Some(out_tx);
                        }

                        // Spawn writer task.
                        let write_arc = Arc::new(TokioMutex::new(write));
                        let write_arc_clone = write_arc.clone();
                        let writer_running = running.clone();
                        let writer_handle = tokio::spawn(async move {
                            while writer_running.load(Ordering::SeqCst) {
                                match out_rx.recv().await {
                                    Some(text) => {
                                        let mut w = write_arc_clone.lock().await;
                                        if w.send(WsMessage::Text(text)).await.is_err() {
                                            break;
                                        }
                                    }
                                    None => break,
                                }
                            }
                        });

                        // Read loop.
                        while let Some(msg_result) = read.next().await {
                            match msg_result {
                                Ok(WsMessage::Text(text)) => {
                                    match serde_json::from_str::<Value>(&text) {
                                        Ok(data) => {
                                            Self::_handle_bridge_message(
                                                &data, &bus_tx, &allow_from,
                                            );
                                        }
                                        Err(_) => {
                                            warn!(
                                                "Invalid JSON from bridge: {}",
                                                &text[..text.len().min(100)]
                                            );
                                        }
                                    }
                                }
                                Ok(WsMessage::Close(_)) => {
                                    info!("WhatsApp bridge closed connection");
                                    break;
                                }
                                Err(e) => {
                                    warn!("WhatsApp WebSocket error: {}", e);
                                    break;
                                }
                                _ => {}
                            }
                        }

                        // Clean up.
                        {
                            let mut slot = ws_tx_slot.lock().await;
                            *slot = None;
                        }
                        writer_handle.abort();
                    }
                    Err(e) => {
                        warn!("WhatsApp bridge connection error: {}", e);
                    }
                }

                if running.load(Ordering::SeqCst) {
                    info!("Reconnecting to WhatsApp bridge in 5 seconds...");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        {
            let mut slot = self.ws_tx.lock().await;
            *slot = None;
        }
        info!("WhatsApp channel stopped");
        Ok(())
    }

    async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let slot = self.ws_tx.lock().await;
        let tx = match slot.as_ref() {
            Some(tx) => tx.clone(),
            None => {
                warn!("WhatsApp bridge not connected");
                return Err(anyhow::anyhow!("WhatsApp bridge not connected"));
            }
        };
        drop(slot);

        let payload = json!({
            "type": "send",
            "to": msg.chat_id,
            "text": msg.content,
        });

        tx.send(serde_json::to_string(&payload).unwrap_or_default())
            .map_err(|e| anyhow::anyhow!("Failed to send WhatsApp message: {}", e))?;

        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}
