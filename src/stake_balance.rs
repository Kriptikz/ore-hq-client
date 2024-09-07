use solana_sdk::{signature::Keypair, signer::Signer};

pub async fn stake_balance(key: &Keypair, url: String, unsecure: bool) {
    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    match client
        .get(format!(
            "{}://{}/miner/stake?pubkey={}",
            url_prefix,
            base_url,
            key.pubkey().to_string()
        ))
        .send()
        .await
    {
        Ok(response) => {
            let balance = response.text().await.unwrap();
            // Check if the balance failed to load
            if balance.contains("Failed to g") {
                println!("  Staked Balance: No staked account");
            } else {
                println!("  Staked Balance: {:.11} ORE", balance);
            }
        }
        Err(e) => {
            println!("  Error fetching stake balance: {:?}", e);
        }
    }
}

pub async fn get_staked_balance(key: &Keypair, url: String, unsecure: bool) -> f64 {
    let base_url = url;
    let client = reqwest::Client::new();
    let url_prefix = if unsecure { "http" } else { "https" };

    match client
        .get(format!(
            "{}://{}/miner/stake?pubkey={}",
            url_prefix,
            base_url,
            key.pubkey().to_string()
        ))
        .send()
        .await
    {
        Ok(response) => {
            let balance_str = response.text().await.unwrap();
            if balance_str.contains("Failed to g") {
                println!("  Delegated stake balance: No staked account");
                0.0
            } else {
                balance_str.parse::<f64>().unwrap_or(0.0)
            }
        }
        Err(e) => {
            println!();
            println!("  Error fetching stake balance: {:?}", e);
            0.0
        }
    }
}
