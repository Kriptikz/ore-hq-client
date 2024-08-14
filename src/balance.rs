use solana_sdk::{signature::Keypair, signer::Signer};

pub async fn balance(key: Keypair, url: String) {
    let base_url = url;
    let client = reqwest::Client::new();

    let balance = client.get(format!("https://{}/miner/balance?pubkey={}", base_url, key.pubkey().to_string())).send().await.unwrap().text().await.unwrap();
    println!("Balance: {}", balance);
}
