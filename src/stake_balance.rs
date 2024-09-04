use solana_sdk::{signature::Keypair, signer::Signer};

pub async fn stake_balance(key: Keypair, url: String, unsecure: bool) {
    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    let balance = client.get(format!("{}://{}/miner/stake?pubkey={}", url_prefix, base_url, key.pubkey().to_string())).send().await.unwrap().text().await.unwrap();
    println!("  Delegated stake balance: {:.11} ORE", balance);
}