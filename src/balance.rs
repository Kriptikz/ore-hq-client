use solana_sdk::{signature::Keypair, signer::Signer};

pub async fn balance(key: &Keypair, url: String, unsecure: bool) {
    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    let balance = client.get(format!("{}://{}/miner/balance?pubkey={}", url_prefix, base_url, key.pubkey().to_string()))
        .send().await.unwrap()
        .text().await.unwrap();

    let rewards = client.get(format!("{}://{}/miner/rewards?pubkey={}", url_prefix, base_url, key.pubkey().to_string()))
        .send().await.unwrap()
        .text().await.unwrap();
    
    println!();
    println!("Wallet Balance: {} ORE", balance);
    println!("Unclaimed Rewards: {} ORE", rewards);
}
