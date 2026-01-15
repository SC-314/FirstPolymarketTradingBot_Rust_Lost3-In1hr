use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use std::collections::VecDeque;
use futures_util::{StreamExt, SinkExt};
use tokio::sync::mpsc::Sender;
use crate::config::VWAP_WINDOW_MS;

#[derive(serde::Deserialize)]
struct BinanceTrade {
    #[serde(rename = "p")] price: String,
    #[serde(rename = "q")] quantity: String,
    #[serde(rename = "E")] time: u64,
}

pub async fn connect(tx: Sender<f64>) -> Result<(), Box<dyn std::error::Error>> {
    let url = "wss://stream.binance.com:9443/ws/btcusdt@trade";
    let (ws_stream, _) = connect_async(url).await.unwrap();
    let (mut write, mut read) = ws_stream.split();

    let mut trades: VecDeque<(u64, f64, f64)> = VecDeque::new();

    let mut total_volume = 0.0;
    let mut price_vol = 0.0;

    while let Some(msg) = read.next().await {

        match msg {
            Ok(Message::Text(text)) => {

                let t: BinanceTrade = match serde_json::from_str(&text) {
                    Ok(t) => t,
                    Err(_) => {
                        continue
                    }
                };

                let (price, quantity, timestamp) = (t.price.parse().unwrap(), t.quantity.parse().unwrap(), t.time);

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
            }
            Ok(Message::Ping(data)) => {
                write.send(Message::Pong(data)).await?;
            }
            Err(e) => {
                println!("FAILED: {e}");
            }
            _ => {
            }
        }
    }
    Ok(())
}