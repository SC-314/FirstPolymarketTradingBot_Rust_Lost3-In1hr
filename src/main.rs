mod get_trend;
use polymarket_client_sdk::POLYGON;
use polymarket_client_sdk::auth::{LocalSigner, Signer};
use polymarket_client_sdk::clob::{Client, Config};
use polymarket_client_sdk::clob::types::SignatureType;
use polymarket_client_sdk::clob::types::request::BalanceAllowanceRequest;
use polymarket_client_sdk::clob::types::AssetType;
use reqwest::get;
use tokio::time::error::Elapsed;
use polymarket_client_sdk::clob::types::{Amount, Side, OrderType};
use tokio_tungstenite::tungstenite::util;
use std::thread;
use polymarket_client_sdk::types::Decimal;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::time::{sleep, Duration};
mod config;
use std::str::FromStr;
mod util_functions;
use crate::util_functions::get_token_ids;
mod get_price_info;

use tokio::sync::watch;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let (tx_trend, mut rx_trend) = watch::channel(0.0);

    tokio::spawn(async move {
        match get_trend::connect(tx_trend).await {
            Ok(_) => {println!("get_trend exited")},
            Err(e) => eprintln!("get_trend failed: {e}"),
        }
    });

    loop {

        let time_now = SystemTime::now().duration_since(UNIX_EPOCH).expect("msg").as_secs();
        let now_900 = time_now % 900;
        let time_15_min = time_now - now_900;

        if now_900 > 870 || now_900 < 10 {
            println!("Market not read yet, waiting 5...");
            sleep(Duration::from_secs(5)).await;
            continue
        }

        let private_key = std::env::var("PRIVATE_KEY").expect("Need a private key");
        let signer = LocalSigner::from_str(&private_key)?.with_chain_id(Some(POLYGON));
        let client = Client::new("https://clob.polymarket.com", Config::default())?
            .authentication_builder(&signer)
            .signature_type(SignatureType::GnosisSafe)
            .funder("0xa11Dae4CE8C30bC6F75334879Cc478E506911D0d".parse()?)
            .authenticate()
            .await?;



        let event_slug = format!("btc-updown-15m-{}", time_15_min);

        let raw_tokens = get_token_ids(&event_slug).await;

        let tokens = match raw_tokens {
            Ok(t) => t,
            Err(e) => {
                eprintln!("ERROR");
                continue;
            }
        };

        let yes_token = tokens[0].clone();
        let no_token = tokens[1].clone();
        println!("yes: {}, no: {}", yes_token, no_token);
        let yes_token_for_ws = yes_token.clone();

        let (tx_price_info, mut rx_price_info) = watch::channel((None::<f64>, None::<f64>));

        tokio::spawn(async move {
            match get_price_info::connect(tx_price_info, &yes_token_for_ws).await {
                Ok(_) => println!("WebSocket connection exited"),
                Err(e) => eprintln!("WebSocket failed: {e}"),
            }
        });

        rx_price_info.changed().await?;



        loop {
            match rx_trend.changed().await {
                Ok(_) => {
                    let trend = *rx_trend.borrow();

                    let limit = 40.0;

                    if trend > limit || trend < -limit {
                        println!("!!! trend: {}", trend);
                        let token = if trend > limit { yes_token.clone() } else { no_token.clone() };
                        let price_info = *rx_price_info.borrow();

                        if let (Some(bid), Some(ask)) = price_info {
                            let price = (bid + ask) / 2.0; 
                            if price > 0.05 && price < 0.95 {

// ------------------------------------------------------------------------------------------------------------------

                                let order = client.market_order().token_id(&token)
                                .amount(Amount::usdc(Decimal::from(1))?).side(Side::Buy).order_type(OrderType::FOK)
                                .build().await?;
                                println!("order built");
                                let signed_order = client.sign(&signer, order).await?;
                                println!("order signed");

                                let response = match client.post_order(signed_order).await {
                                    Ok(resp) => resp,
                                    Err(e) => {
                                        println!("Error buy posting order: {}", e);
                                        continue
                                    }
                                };

                                println!("Order response: {:?}", response);

                                if response.success {
                                    let taking_amount = response.taking_amount.to_string().parse::<f64>().ok().unwrap_or(0.0);
                                    let mut amount_to_sell = (taking_amount * 100.0).floor() / 100.0;

                                    sleep(Duration::from_secs(3)).await;

                                    loop {
                                        let amount_str = format!("{:.2}", amount_to_sell);
                                        let amount_dec = Decimal::from_str(&amount_str).unwrap();

                                        println!("{}", amount_dec);

                                        let order = client.market_order().token_id(&token)
                                        .amount(Amount::shares(amount_dec)?).side(Side::Sell).order_type(OrderType::FAK)
                                        .build().await?;
                                        println!("order built");
                                    
                                        let signed_order = client.sign(&signer, order).await?;
                                        println!("order signed");

                                        let response = match client.post_order(signed_order).await {
                                            Ok(resp) => resp,
                                            Err(e) => {
                                                amount_to_sell -= 0.01;
                                                if amount_to_sell <= 0.01 {
                                                    break;
                                                }
                                                println!("Error posting sell order: {}", e);
                                                sleep(Duration::from_millis(500)).await;
                                                continue;
                                            }
                                        };
                                        println!("Order response: {:?}", response);

                                        if response.success {
                                            let making_amount = response.making_amount.to_string().parse::<f64>().ok().unwrap_or(0.0);
                                            amount_to_sell -= (making_amount * 100.0).floor() / 100.0;
                                            // let new_amount_str = format!("{:.2}", amount_to_sell);
                                            // let amount_dec = Decimal::from_str(&new_amount_str).unwrap();
                                            if amount_to_sell <= 0.01 {
                                                println!("Full Sell Fill");
                                                sleep(Duration::from_secs(3)).await;
                                                break;
                                            } else {
                                                println!("Partial Sell Fill");
                                            }
                                        } else {
                                            println!("Sell order failed");
                                            break;
                                        }
                                    }
                                }


// ------------------------------------------------------------------------------------------------------------------

                            }
                        }

                    }
                    let time_now = SystemTime::now().duration_since(UNIX_EPOCH).expect("msg").as_secs();
                    let now_900 = time_now % 900;

                    if now_900 > 870 || now_900 < 10 {
                        println!("Exiting current event");
                        break;
                    }
                },
                Err(_) => {
                    println!("trend channel closed");
                    break;
                }
            }
        }
    }


}
