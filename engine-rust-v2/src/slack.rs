// ============================================================
// SLACK NOTIFICATION MODULE
// ============================================================

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Slack message payload
#[derive(Debug, Serialize)]
struct SlackPayload {
    channel: String,
    username: String,
    icon_emoji: String,
    attachments: Vec<SlackAttachment>,
}

#[derive(Debug, Serialize)]
struct SlackAttachment {
    color: String,
    title: String,
    text: String,
    fields: Vec<SlackField>,
    footer: String,
    ts: i64,
}

#[derive(Debug, Serialize)]
struct SlackField {
    title: String,
    value: String,
    short: bool,
}

/// Slack configuration
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub enabled: bool,
    pub webhook_url: String,
    pub channel: String,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            webhook_url: String::new(),
            channel: "#trading".to_string(),
        }
    }
}

/// Thread-safe Slack client with dynamic configuration
pub struct SlackClient {
    enabled: Arc<AtomicBool>,
    webhook_url: String,
    channel: Arc<std::sync::RwLock<String>>,
    client: reqwest::blocking::Client,
}

impl SlackClient {
    /// Create new Slack client with configuration
    pub fn new(enabled: bool, webhook_url: String, channel: String) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
            webhook_url,
            channel: Arc::new(std::sync::RwLock::new(channel)),
            client,
        }
    }

    /// Create from config struct (reserved for future use)
    #[allow(dead_code)]
    pub fn from_config(config: &SlackConfig) -> Self {
        Self::new(config.enabled, config.webhook_url.clone(), config.channel.clone())
    }

    /// Enable/disable Slack notifications at runtime (reserved for future use)
    #[allow(dead_code)]
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    /// Check if Slack is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Toggle Slack on/off (reserved for future use)
    #[allow(dead_code)]
    pub fn toggle(&self) -> bool {
        let new_state = !self.enabled.load(Ordering::SeqCst);
        self.enabled.store(new_state, Ordering::SeqCst);
        new_state
    }

    /// Update channel at runtime (reserved for future use)
    #[allow(dead_code)]
    pub fn set_channel(&self, channel: String) {
        if let Ok(mut guard) = self.channel.write() {
            *guard = channel;
        }
    }

    /// Get current channel
    pub fn get_channel(&self) -> String {
        self.channel.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Check if webhook is configured
    pub fn is_configured(&self) -> bool {
        !self.webhook_url.is_empty()
    }
    
    /// Send trade signal notification
    pub fn send_signal(
        &self,
        direction: &str,
        entry_price: f64,
        sl: f64,
        tp: f64,
        score: i32,
        confidence: f64,
        reason: &str,
    ) -> Result<(), String> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let color = match direction {
            "BUY" => "#00FF00",  // Green
            "SELL" => "#FF0000", // Red
            _ => "#FFFF00",       // Yellow
        };

        let emoji = match direction {
            "BUY" => ":large_green_circle:",
            "SELL" => ":red_circle:",
            _ => ":warning:",
        };

        let title = format!("{} {} @ {:.2}", emoji, direction, entry_price);

        let attachment = SlackAttachment {
            color: color.to_string(),
            title,
            text: reason.to_string(),
            fields: vec![
                SlackField {
                    title: "Entry".to_string(),
                    value: format!("{:.2}", entry_price),
                    short: true,
                },
                SlackField {
                    title: "SL".to_string(),
                    value: format!("{:.2}", sl),
                    short: true,
                },
                SlackField {
                    title: "TP".to_string(),
                    value: format!("{:.2}", tp),
                    short: true,
                },
                SlackField {
                    title: "Score".to_string(),
                    value: format!("{}/10", score),
                    short: true,
                },
                SlackField {
                    title: "Confidence".to_string(),
                    value: format!("{:.0}%", confidence * 100.0),
                    short: true,
                },
                SlackField {
                    title: "Risk:Reward".to_string(),
                    value: format!("1:{:.1}", (tp - entry_price).abs() / (entry_price - sl).abs()),
                    short: true,
                },
            ],
            footer: "GOLD Scalping Bot v2.0".to_string(),
            ts: chrono::Utc::now().timestamp(),
        };

        self.send_attachment(attachment)
    }

    /// Send position closed notification (backward compatible)
    pub fn send_position_closed(
        &self,
        ticket: &str,
        direction: &str,
        volume: f64,
        price: f64,
        profit: f64,
        magic: i32,
    ) -> Result<(), String> {
        self.send_position_closed_with_reason(ticket, direction, volume, price, profit, magic, "CLOSE")
    }

    /// Send position closed notification with reason (TP/SL/MANUAL/BATCH)
    pub fn send_position_closed_with_reason(
        &self,
        ticket: &str,
        direction: &str,
        volume: f64,
        price: f64,
        profit: f64,
        magic: i32,
        reason: &str,
    ) -> Result<(), String> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let (color, emoji, reason_emoji) = match (profit >= 0.0, reason) {
            (true, "TP") => ("#00FF00", ":trophy:", ":trophy:"),
            (true, _) => ("#00FF00", ":white_check_mark:", ":chart_with_upwards_trend:"),
            (false, "SL") => ("#FF0000", ":skull:", ":fire:"),
            (false, _) => ("#FF0000", ":x:", ":chart_with_downwards_trend:"),
        };

        let pnl_text = format!("${:.2}", profit);
        let reason_label = match reason {
            "TP" => "TAKE PROFIT",
            "SL" => "STOP LOSS",
            "MANUAL" => "MANUAL CLOSE",
            "BATCH" => "BATCH CLOSE",
            _ => "CLOSED",
        };

        let attachment = SlackAttachment {
            color: color.to_string(),
            title: format!("{} POSITION {}: {} {} lots @ {:.2}", emoji, reason_label, direction, volume, price),
            text: format!("Ticket: {} | Magic: {} | P&L: {} {}", ticket, magic, reason_emoji, pnl_text),
            fields: vec![
                SlackField {
                    title: "Direction".to_string(),
                    value: direction.to_string(),
                    short: true,
                },
                SlackField {
                    title: "Volume".to_string(),
                    value: format!("{}", volume),
                    short: true,
                },
                SlackField {
                    title: "Price".to_string(),
                    value: format!("{:.2}", price),
                    short: true,
                },
                SlackField {
                    title: "P&L".to_string(),
                    value: format!("{:.2}", profit),
                    short: true,
                },
                SlackField {
                    title: "Close Reason".to_string(),
                    value: reason_label.to_string(),
                    short: true,
                },
                SlackField {
                    title: "Magic".to_string(),
                    value: format!("{}", magic),
                    short: true,
                },
            ],
            footer: "GOLD Scalping Bot v2.0".to_string(),
            ts: chrono::Utc::now().timestamp(),
        };

        self.send_attachment(attachment)
    }

    /// Send order execution notification
    pub fn send_order_executed(
        &self,
        direction: &str,
        volume: f64,
        price: f64,
        order_id: &str,
    ) -> Result<(), String> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let color = "#00FF00";
        let emoji = ":white_check_mark:";

        let attachment = SlackAttachment {
            color: color.to_string(),
            title: format!("{} ORDER EXECUTED: {} {} lots @ {:.2}", emoji, direction, volume, price),
            text: format!("Order ID: {}", order_id),
            fields: vec![],
            footer: "GOLD Scalping Bot v2.0".to_string(),
            ts: chrono::Utc::now().timestamp(),
        };

        self.send_attachment(attachment)
    }

    /// Send order request notification (before sending to MT5) - useful to track attempted opens
    pub fn send_order_request(&self, direction: &str, volume: f64, price: f64, sl: Option<f64>, tp: Option<f64>, request_id: &str) -> Result<(), String> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let color = "#FFD700"; // gold
        let emoji = ":arrow_forward:";

        let mut fields = vec![
            SlackField { title: "Direction".to_string(), value: direction.to_string(), short: true },
            SlackField { title: "Volume".to_string(), value: format!("{:.2}", volume), short: true },
            SlackField { title: "Price".to_string(), value: format!("{:.2}", price), short: true },
            SlackField { title: "RequestId".to_string(), value: request_id.to_string(), short: true },
        ];

        if let Some(slv) = sl {
            fields.push(SlackField { title: "SL".to_string(), value: format!("{:.2}", slv), short: true });
        }
        if let Some(tpv) = tp {
            fields.push(SlackField { title: "TP".to_string(), value: format!("{:.2}", tpv), short: true });
        }

        let attachment = SlackAttachment {
            color: color.to_string(),
            title: format!("{} ORDER REQUEST: {} {} lots", emoji, direction, volume),
            text: format!("Request id: {}", request_id),
            fields,
            footer: "GOLD Scalping Bot v2.0".to_string(),
            ts: chrono::Utc::now().timestamp(),
        };

        self.send_attachment(attachment)
    }

    /// Send error/alert notification (reserved for future use)
    #[allow(dead_code)]
    pub fn send_alert(&self, title: &str, message: &str, is_error: bool) -> Result<(), String> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let color = if is_error { "#FF0000" } else { "#FFFF00" };
        let emoji = if is_error { ":x:" } else { ":warning:" };

        let attachment = SlackAttachment {
            color: color.to_string(),
            title: format!("{} {}", emoji, title),
            text: message.to_string(),
            fields: vec![],
            footer: "GOLD Scalping Bot v2.0".to_string(),
            ts: chrono::Utc::now().timestamp(),
        };

        self.send_attachment(attachment)
    }

    /// Send status/heartbeat notification
    pub fn send_status(&self, message: &str) -> Result<(), String> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let attachment = SlackAttachment {
            color: "#808080".to_string(),
            title: ":heartbeat: Bot Status".to_string(),
            text: message.to_string(),
            fields: vec![],
            footer: "GOLD Scalping Bot v2.0".to_string(),
            ts: chrono::Utc::now().timestamp(),
        };

        self.send_attachment(attachment)
    }

    /// Send optimizer update notification
    pub fn send_optimizer_update(&self, title: &str, result_text: &str) -> Result<(), String> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let attachment = SlackAttachment {
            color: "#2E86FF".to_string(),
            title: format!(":gear: {}", title),
            text: result_text.to_string(),
            fields: vec![],
            footer: "GOLD Scalping Bot v2.0".to_string(),
            ts: chrono::Utc::now().timestamp(),
        };

        self.send_attachment(attachment)
    }

    /// Send custom message with custom color (reserved for future use)
    #[allow(dead_code)]
    pub fn send_custom(
        &self,
        title: &str,
        message: &str,
        color: &str,
        emoji: &str,
    ) -> Result<(), String> {
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let attachment = SlackAttachment {
            color: color.to_string(),
            title: format!("{} {}", emoji, title),
            text: message.to_string(),
            fields: vec![],
            footer: "GOLD Scalping Bot v2.0".to_string(),
            ts: chrono::Utc::now().timestamp(),
        };

        self.send_attachment(attachment)
    }

    fn send_attachment(&self, attachment: SlackAttachment) -> Result<(), String> {
        if self.webhook_url.is_empty() {
            return Err("Slack webhook URL not configured".to_string());
        }

        let channel = self.get_channel();

        let payload = SlackPayload {
            channel,
            username: "GOLD Bot".to_string(),
            icon_emoji: ":chart_with_upwards_trend:".to_string(),
            attachments: vec![attachment],
        };

        let json = serde_json::to_string(&payload)
            .map_err(|e| format!("Failed to serialize payload: {}", e))?;

        self.client
            .post(&self.webhook_url)
            .header("Content-Type", "application/json")
            .body(json)
            .send()
            .map_err(|e| format!("Failed to send to Slack: {}", e))?;

        Ok(())
    }
}

/// Test Slack connection (for debugging)
#[allow(dead_code)]
pub fn test_slack_connection(webhook_url: &str, channel: &str) -> Result<(), String> {
    let client = SlackClient::new(true, webhook_url.to_string(), channel.to_string());
    client.send_alert("Test Message", "GOLD Bot is connected and working!", false)
}

/// Runtime control for Slack via ZMQ commands (reserved for future use)
#[allow(dead_code)]
#[derive(Debug)]
pub enum SlackCommand {
    Enable,
    Disable,
    Toggle,
    SetChannel(String),
    Status,
}

impl SlackClient {
    /// Process control command and return response message (reserved for future use)
    #[allow(dead_code)]
    pub fn process_command(&self, cmd: SlackCommand) -> String {
        match cmd {
            SlackCommand::Enable => {
                self.set_enabled(true);
                format!("Slack notifications ENABLED (channel: {})", self.get_channel())
            }
            SlackCommand::Disable => {
                self.set_enabled(false);
                "Slack notifications DISABLED".to_string()
            }
            SlackCommand::Toggle => {
                let new_state = self.toggle();
                format!("Slack notifications {}", if new_state { "ENABLED" } else { "DISABLED" })
            }
            SlackCommand::SetChannel(channel) => {
                let old = self.get_channel();
                self.set_channel(channel.clone());
                format!("Channel changed: {} -> {}", old, channel)
            }
            SlackCommand::Status => {
                format!(
                    "Slack Status: {} | Channel: {} | Configured: {}",
                    if self.is_enabled() { "ENABLED" } else { "DISABLED" },
                    self.get_channel(),
                    if self.is_configured() { "YES" } else { "NO" }
                )
            }
        }
    }
}
