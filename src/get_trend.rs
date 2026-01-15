use tokio::sync::watch;
use tokio::sync::mpsc;
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Instant, Duration};
pub mod binance;
pub mod coinbase;
pub mod kraken;
pub mod bitget;
pub mod okx;



use crate::config;


fn trend_slope(data: &VecDeque<(u64, f64)>) -> f64 {
    let n = data.len() as f64;
    if n < 2.0 {
        return 0.0;
    }

    let t0 = data.front().unwrap().0;

    let mut sum_t = 0.0f64;
    let mut sum_p = 0.0f64;
    let mut sum_tt = 0.0f64;
    let mut sum_tp = 0.0f64;

    for &(t, p) in data {
        let t = (t - t0) as f64 * 0.0001; // normalize + convert
        sum_t += t;
        sum_p += p;
        sum_tt += t * t;
        sum_tp += t * p;
    }

    let denom = n * sum_tt - sum_t * sum_t;
    if denom == 0.0 {
        return 0.0;
    }

    (n * sum_tp - sum_t * sum_p) / denom
}


pub async fn connect(tx_out: watch::Sender<f64>) -> Result<(), Box<dyn std::error::Error>> {
    let (tx_binance, mut rx_binance) = mpsc::channel::<f64>(1024);
    let (tx_coinbase, mut rx_coinbase) = mpsc::channel::<f64>(1024);
    let (tx_kraken, mut rx_kraken) = mpsc::channel::<f64>(1024);
    let (tx_bitget, mut rx_bitget) = mpsc::channel::<f64>(1024);
    let (tx_okx, mut rx_okx) = mpsc::channel::<f64>(1024);

    let mut warmup_start: Option<Instant> = None;

    let mut prices = [0.0,0.0,0.0,0.0,0.0];

    let weights: [f64; 5] = [0.354, 0.308, 0.059, 0.104, 0.175]; // using 24hr trading volume

    let mut price_times: VecDeque<(u64, f64)> = VecDeque::new();

    tokio::spawn(async move {
        let _ = binance::connect(tx_binance).await;
    });
    tokio::spawn(async move {
        let _ = coinbase::connect(tx_coinbase).await;
    });
    tokio::spawn(async move {
        let _ = kraken::connect(tx_kraken).await;
    });
    tokio::spawn(async move {
        let _ = bitget::connect(tx_bitget).await;
    });
    tokio::spawn(async move {
        let _ = okx::connect(tx_okx).await;
    });

    let mut price_updates: [Option<Instant>; 5] = [None; 5];

    loop {

        let now = Instant::now();

        tokio::select! {
            Some(binance_vwap) = rx_binance.recv() => {
                prices[0] = binance_vwap;
                price_updates[0] = Some(now);
            }
            Some(coinbase_vwap) = rx_coinbase.recv() => {
                prices[1] = coinbase_vwap;
                price_updates[1] = Some(now);
            }
            Some(kraken_vwap) = rx_kraken.recv() => {
                prices[2] = kraken_vwap;
                price_updates[2] = Some(now);
            }
            Some(bitget_vwap) = rx_bitget.recv() => {
                prices[3] = bitget_vwap;
                price_updates[3] = Some(now);
            }
            Some(okx_vwap) = rx_okx.recv() => {
                prices[4] = okx_vwap;
                price_updates[4] = Some(now);
            }
            else => break,
        }

        let mut weighted_sum = 0.0;
        let mut weight_sum = 0.0;
        let mut count = 0;

        for i in 0..5 {
            if let Some(update_time) = price_updates[i] {
                if now.duration_since(update_time).as_secs() < 5 {
                    let w = weights[i];
                    weighted_sum += prices[i] * w;
                    weight_sum += w;
                    count += 1;
                }
            }
        }

        if count < 4 {
            continue;
        }


        let new_price = weighted_sum / weight_sum as f64;

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        price_times.push_back((timestamp, new_price));
        

        let window_start = timestamp.saturating_sub(config::LIN_BEST_FIT_MS);

        while let Some((time, _)) = price_times.front() {
            if *time < window_start {
                price_times.pop_front(); 
            } else {
                break;
            }
        }

        let trend = trend_slope(&price_times);

        if warmup_start.is_none() {
            println!("Waiting 5 seconds to get mean");
            warmup_start = Some(Instant::now());
            continue;
        }

        if warmup_start.unwrap().elapsed() < Duration::from_secs(5) {
            continue;
        }

        let _ = tx_out.send(trend);
    }
    Ok(())
}
