use futures::StreamExt as _;
use polymarket_client_sdk::clob::ws::Client;
use tokio::sync::watch::Sender;

pub async fn connect(
    tx: Sender<(Option<f64>, Option<f64>)>,
    asset_id: &String,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::default();
    
    let asset_ids = vec![asset_id.clone()];
    let stream = client.subscribe_orderbook(asset_ids)?;
    let mut stream = Box::pin(stream);

    println!("WebSocket connected for asset: {}", asset_id);

    while let Some(book_result) = stream.next().await {
        let book = book_result?;

        // println!("Update for {}: {} bids, {} asks | Bid: {:.4}, Ask: {:.4}",
        //     book.asset_id, book.bids.len(), book.asks.len(), best_bid, best_ask);
        
        if let (Some(best_bid), Some(best_ask)) = 
            (book.bids.last(), book.asks.last()) {
                let (best_bid, best_ask) =
                (best_bid.price.to_string().parse::<f64>().ok(), best_ask.price.to_string().parse::<f64>().ok());

                let _ = tx.send((best_bid, best_ask));
            }

        // Send the prices through the channel
        // let _ = tx.send((best_bid, best_ask));
    }

    Ok(())
}