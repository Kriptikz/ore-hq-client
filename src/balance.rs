use solana_sdk::{signature::Keypair, signer::Signer};

pub async fn balance(key: &Keypair, url: String, unsecure: bool) {
    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    // Fetch Wallet (Stakable) Balance
    let balance_response = client
        .get(format!(
            "{}://{}/miner/balance?pubkey={}",
            url_prefix,
            base_url,
            key.pubkey().to_string()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let balance = match balance_response.parse::<f64>() {
        Ok(b) => b,
        Err(_) => 0.0,
    };

    // Fetch Unclaimed Rewards
    let rewards_response = client
        .get(format!(
            "{}://{}/miner/rewards?pubkey={}",
            url_prefix,
            base_url,
            key.pubkey().to_string()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let rewards = match rewards_response.parse::<f64>() {
        Ok(r) => r,
        Err(_) => 0.0,
    };

    // Fetch Staked Balance
    let stake_response = client
        .get(format!(
            "{}://{}/miner/stake?pubkey={}",
            url_prefix,
            base_url,
            key.pubkey().to_string()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let staked_balance = if stake_response.contains("Failed to g") {
        println!("  Delegated stake balance: No staked account");
        0.0
    } else {
        stake_response.parse::<f64>().unwrap_or(0.0)
    };
    println!("  Unclaimed Rewards: {:.11} ORE", rewards);
    println!("  Wallet (Stakable): {:.11} ORE", balance);
    println!("  Staked Balance:    {:.11} ORE", staked_balance);
}

pub async fn get_balance(key: &Keypair, url: String, unsecure: bool) -> f64 {
    let client = reqwest::Client::new();
    let url_prefix = if unsecure { "http" } else { "https" };

    let balance_response = client
        .get(format!(
            "{}://{}/miner/balance?pubkey={}",
            url_prefix,
            url,
            key.pubkey().to_string()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    balance_response.parse::<f64>().unwrap_or(0.0)
}
