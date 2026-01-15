use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use serde_json::json;
use futures_util::{StreamExt, SinkExt};
use tokio::sync::mpsc::Sender;
use std::collections::VecDeque;
use crate::config::VWAP_WINDOW_MS;

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct KrakenTrade(
    String,
    String,
    String,
    String,
    String,
    String
);

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct KrakenTradeMessage(
    i64,
    Vec<KrakenTrade>,
    String,
    String
);

pub async fn connect(tx: Sender<f64>) -> Result<(), Box<dyn std::error::Error>> {
    let url = "wss://ws.kraken.com";

    let (ws_stream, _) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();
    
    let subscribe_msg = json!({
        "event": "subscribe",
        "pair": ["XBT/USD"],
        "subscription": {
            "name": "trade"
        }
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

                let t: KrakenTradeMessage = match serde_json::from_str(&text) {
                    Ok(t) => t,
                    Err(_) => {
                        // println!("faield: {}", e);
                        // println!(" FAIL {}", text);
                        continue
                    }
                };
                // println!(" SUCCESS {:?}", t);
                for trade in &t.1 {

                    let (price, quantity, timestamp) = (trade.0.parse::<f64>().unwrap(), trade.1.parse::<f64>().unwrap(), (trade.2.parse::<f64>().unwrap() * 1000.0) as u64);                
                    
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
                        // println!("{price_vol}");
                        let vwap = price_vol / total_volume;
                        if tx.send(vwap).await.is_err() {
                            break;
                        }
                    }

                }

                
                // println!("{}, {}, {}", price, quantity, timestamp)
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