use solana_sdk::{signature::Keypair, signer::Signer};
use std::io::Read;
use std::time::Duration;
use std::thread::sleep;

pub async fn balance(key: &Keypair, url: String, unsecure: bool) {
    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

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

    println!();
    println!("  Unclaimed Rewards: {:.11} ORE", rewards);
    println!("  Wallet (Stakable): {:.11} ORE", balance);
}

fn prompt_to_continue() {
    sleep(Duration::from_millis(100));
    println!("\n  Press any key to continue...");
    let _ = std::io::stdin().read(&mut [0u8]).unwrap();
}