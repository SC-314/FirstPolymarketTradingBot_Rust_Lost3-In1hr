use serde_json::Value;

pub async fn get_token_ids(event_slug: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    
    let url = format!("https://gamma-api.polymarket.com/events/slug/{}", event_slug);
    
    let client = reqwest::Client::new();
    
    let response = client.get(url).send().await?.json::<Value>().await?;

    if let Some(raw_tokens_string) = response["markets"][0]["clobTokenIds"].as_str() {
        
        if response["markets"][0]["slug"].as_str() == Some(event_slug) {
            
            let tokens: Vec<String> = serde_json::from_str(raw_tokens_string)?;
            return Ok(tokens);
            
        } else {
            return Err("Wrong market found: Slug mismatch".into());
        }
    } else {
        return Err("No markets have been found or clobTokenIds are missing".into());
    }
}