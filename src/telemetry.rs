use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::accept_async;
use tungstenite::Message;
use tracing::{info, error, warn};


#[derive(Debug, Clone, Serialize)]
pub struct TelemetryPacket {
    pub msg_type: String,   // "TRADE" | "BOOK"
    pub timestamp: u64,     // 事件时间

    // --- 交易特有字段 (Book 消息通常为 None) ---
    pub price: Option<f64>,
    pub quantity: Option<f64>,
    pub is_buyer_maker: Option<bool>,

    // --- 通用指标字段 (根据 msg_type 决定其含义) ---
    // 如果是 TRADE: 代表 Trade Vol, Flow Imbalance, VWAP Bias
    // 如果是 BOOK:  代表 Book Vol,  Order Book Imbalance, WMP Bias
    pub vol: Option<f64>,
    pub imbalance: Option<f64>,
    pub bias: Option<f64>,
    pub trend_state: Option<i8>,
}

// --- 遥测服务 ---
pub struct TelemetryServer {
    tx: broadcast::Sender<String>,
    enabled: bool,
}

impl TelemetryServer {
    /// 创建并根据配置决定是否启动
    pub fn new(enabled: bool, port: u16) -> Self {
        // 创建广播通道，容量设为 2000。
        // 原理：这是一个环形缓冲区。
        // 如果 Python 消费太慢，旧数据会被覆盖，Rust 发送端永远不会阻塞。
        let (tx, _rx) = broadcast::channel(2000);

        if enabled {
            let tx_clone = tx.clone();

            // 启动异步任务监听端口
            tokio::spawn(async move {
                let addr = format!("127.0.0.1:{}", port);
                match TcpListener::bind(&addr).await {
                    Ok(listener) => {
                        info!("📡 [Telemetry] Server running on ws://{}", addr);

                        // 循环接受 TCP 连接
                        while let Ok((stream, _)) = listener.accept().await {
                            let tx_inner = tx_clone.clone();
                            // 为每个连接生成的 Python 客户端启动一个独立任务
                            tokio::spawn(async move {
                                handle_connection(stream, tx_inner).await;
                            });
                        }
                    }
                    Err(e) => {
                        error!("❌ [Telemetry] Failed to bind port {}: {}", port, e);
                    }
                }
            });
        } else {
            info!("📡 [Telemetry] Disabled by config.");
        }

        Self { tx, enabled }
    }

    /// 发送数据接口 (极快，纳秒级)
    pub fn send(&self, packet: TelemetryPacket) {
        if !self.enabled {
            return;
        }

        // 只有当有接收者(Python已连接)时才进行序列化，节省 CPU
        if self.tx.receiver_count() > 0 {
            if let Ok(msg) = serde_json::to_string(&packet) {
                // send 可能会返回错误(如果没有接收者)，忽略即可
                let _ = self.tx.send(msg);
            }
        }
    }
}

/// 处理单个 WebSocket 连接
async fn handle_connection(stream: tokio::net::TcpStream, tx: broadcast::Sender<String>) {
    // 1. 将 TCP 升级为 WebSocket
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!("WebSocket handshake failed: {}", e);
            return;
        }
    };

    let (mut ws_sender, _ws_receiver) = ws_stream.split();

    // 2. 订阅广播通道
    let mut rx = tx.subscribe();

    // 3. 循环接收广播并转发
    loop {
        match rx.recv().await {
            Ok(msg) => {
                // 发送 Text Frame
                if let Err(_) = ws_sender.send(Message::Text(msg.into())).await {
                    // 发送失败意味着客户端断开
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Python 端处理太慢，导致丢包。
                // 这在 HFT 监控中是正常的，直接跳过，不用管。
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}