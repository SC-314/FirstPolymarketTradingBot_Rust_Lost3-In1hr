use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use serde_json::json;
use futures_util::{StreamExt, SinkExt};
use tokio::sync::mpsc::Sender;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;

#[derive(serde::Deserialize)]
struct CoinbaseTrade {
    #[serde(rename = "price")] price: String,
    #[serde(rename = "size")] quantity: String,
    #[serde(rename = "time")] time: DateTime<Utc>
}

pub async fn connect(tx: Sender<f64>) -> Result<(), Box<dyn std::error::Error>> {
    let url = "wss://ws-feed.exchange.coinbase.com";

    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    let subscribe_msg = json!({
        "type": "subscribe",
        "product_ids": ["BTC-USD"],
        "channels": ["matches"]
    });
    
    write
        .send(Message::Text(subscribe_msg.to_string().into()))
        .await?;

    let mut trades: VecDeque<(u64, f64, f64)> = VecDeque::new();

    let window_ms = 1000 as u64;
    let mut total_volume = 0.0;
    let mut price_vol = 0.0;

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let t: CoinbaseTrade = match serde_json::from_str(&text) {
                    Ok(t) => t,
                    Err(_) => {
                        continue
                    }
                };
                let (price, quantity, timestamp) = (t.price.parse().unwrap(), t.quantity.parse().unwrap(), (t.time.timestamp_millis()) as u64);
                // println!("{}, {}, {}", price, quantity, timestamp);

                total_volume += quantity;
                price_vol += price * quantity;
                trades.push_back((timestamp, price, quantity));

                let window_start = timestamp.saturating_sub(window_ms);

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