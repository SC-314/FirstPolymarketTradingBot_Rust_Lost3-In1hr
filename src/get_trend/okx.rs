use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use serde_json::json;
use futures_util::{StreamExt, SinkExt};
use tokio::sync::mpsc::Sender;
use std::collections::VecDeque;
use crate::config::VWAP_WINDOW_MS;


#[derive(Debug, serde::Deserialize)]
struct OkxTradeMessage {
    data: Vec<OkxTrade>,
}

#[derive(Debug, serde::Deserialize)]
struct OkxTrade {
    #[serde(rename = "px")] price: String,
    #[serde(rename = "sz")] size: String,
    #[serde(rename = "ts")] time: String,
}

pub async fn connect(tx: Sender<f64>) -> Result<(), Box<dyn std::error::Error>> {
    let url = "wss://ws.okx.com:8443/ws/v5/public";

    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();
    
    let subscribe_msg = json!({
        "op": "subscribe",
        "args": [
            {
            "channel": "trades",
            "instId": "BTC-USDT"
            }
        ]
});
    
    write
        .send(Message::Text(subscribe_msg.to_string().into()))
        .await?;

    let mut trades: VecDeque<(u64, f64, f64)> = VecDeque::new();

    let mut total_volume = 0.0;
    let mut price_vol = 0.0;

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let t: OkxTradeMessage = match serde_json::from_str(&text) {
                    Ok(t) => t,
                    Err(_) => {
                        continue
                    }
                };
                for trade in &t.data {

                    let (price, quantity, timestamp) = (trade.price.parse::<f64>().unwrap(), trade.size.parse::<f64>().unwrap(), (trade.time.parse::<f64>().unwrap()) as u64);                
                    
                    total_volume += quantity;
                    price_vol += price * quantity;
                    trades.push_back((timestamp, price, quantity));

                    let window_start = timestamp.saturating_sub(VWAP_WINDOW_MS);

                    while let Some((time, price, quantity)) = trades.front() {
                        if *time < window_start {
                            price_vol -= price * quantity;
                            total_volume -= quantity;
                            trades.pop_front(); 
                        } else {
                            break;
                        }
                    }
                    if total_volume > 0.0 {
                        let vwap = price_vol / total_volume;
                        if tx.send(vwap).await.is_err() {
                            break;
                        }
                    }
                    // println!("{}, {}, {}", price, quantity, timestamp);
                }
            }
            Ok(Message::Ping(data)) => {
                write.send(Message::Pong(data)).await?;
            }
            Err(e) => {
                eprintln!("WebSocket error: {e}");
                break;
            }
            _ => {}
        }
    }

    Ok(())
}