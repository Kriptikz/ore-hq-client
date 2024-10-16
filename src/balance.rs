use solana_sdk::{signature::Keypair, signer::Signer};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

pub async fn balance(key: &Keypair, url: String, unsecure: bool) {
    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    // Fetch Wallet (Stakeable) Balance
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

    println!();
    println!("  Unclaimed Rewards: {:.11} ORE", rewards);
    println!("  Staked Balance:    {:.11} ORE", staked_balance);
    println!();

    let token_mints: HashMap<&str, &str> = HashMap::from([
        ("oreoU2P8bN6jkk3jbaiVxYnG1dCXcYxwhwyK9jSybcp", "ORE Token"),
        ("DrSS5RM7zUd9qjUEdDaf31vnDUSbCrMto6mjqTrHFifN", "ORE-SOL LP"),
        ("meUwDp23AaxhiNKaQCyJ2EAF2T4oe1gSkEkGXSRVdZb", "ORE-ISC LP"),
    ]);

    print!("In Wallet (Stakeable):\n");
    for (mint, label) in token_mints.iter() {
        let token_balance =
            get_token_balance(key, base_url.clone(), unsecure, mint.to_string()).await;
        println!("  {}: {}", label, token_balance);
    }
    println!();
    println!("Boosted:");
    for (mint, label) in token_mints.iter() {
        let boosted_token_balance =
            get_boosted_stake_balance(key, base_url.clone(), unsecure, mint.to_string()).await;
        println!("  {}: {}", label, boosted_token_balance);
    }
}

pub async fn get_token_balance(key: &Keypair, url: String, unsecure: bool, mint: String) -> f64 {
    let client = reqwest::Client::new();
    let url_prefix = if unsecure { "http" } else { "https" };

    let balance_response = client
        .get(format!(
            "{}://{}/v2/miner/balance?pubkey={}&mint={}",
            url_prefix,
            url,
            key.pubkey().to_string(),
            mint
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    balance_response.parse::<f64>().unwrap_or(0.0)
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

pub async fn get_boosted_stake_balance(
    key: &Keypair,
    url: String,
    unsecure: bool,
    mint: String,
) -> f64 {
    let client = reqwest::Client::new();
    let url_prefix = if unsecure { "http" } else { "https" };

    let balance_response = client
        .get(format!(
            "{}://{}/miner/boost/stake?pubkey={}&mint={}",
            url_prefix,
            url,
            key.pubkey().to_string(),
            mint
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    balance_response.parse::<f64>().unwrap_or(0.0)
}
