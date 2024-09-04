use solana_sdk::{signature::Keypair, signer::Signer};
use reqwest::Error;

pub async fn stake_balance(key: Keypair, url: String, unsecure: bool) {
    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    // Fetch balance
    match client.get(format!("{}://{}/miner/stake?pubkey={}", url_prefix, base_url, key.pubkey().to_string()))
        .send().await
        {
        Ok(response) => {
            let balance = response.text().await.unwrap();
            // Check if the balance failed to load
            if balance.contains("Failed to g") {
                println!("  Delegated stake balance: No staked account");
            } else {
                println!("  Delegated stake balance: {:.11} ORE", balance);
            }
        },
        Err(e) => {
            // Handle request failure
            println!("  Error fetching stake balance: {:?}", e);
        }
    }
}
