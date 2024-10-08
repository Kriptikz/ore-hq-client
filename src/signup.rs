use std::str::FromStr;

use base64::{prelude::BASE64_STANDARD, Engine};
use clap::Parser;
use solana_sdk::{
    pubkey::Pubkey, signature::Keypair, signer::Signer, system_instruction,
    transaction::Transaction,
};

#[derive(Debug, Parser)]
pub struct SignupArgs {
    #[arg(
        long,
        value_name = "PUBKEY",
        default_value = None,
        help = "Miner public key to enable."
    )]
    pub pubkey: Option<String>,
}

pub async fn signup(args: SignupArgs, url: String, key: Keypair, unsecure: bool) {
    let miner_pubkey = if args.pubkey.is_some() {
        match Pubkey::from_str(&args.pubkey.unwrap()) {
            Ok(pk) => {
                pk
            },
            Err(e) => {
                println!("Invalid miner pubkey arg provided.");
                return;
            }
        }
    } else {
            key.pubkey()
    };

    let base_url = url;

    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    let resp = client
        .get(format!(
            "{}://{}/pool/authority/pubkey",
            url_prefix, base_url
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let pool_pubkey = Pubkey::from_str(&resp).unwrap();

    let resp = client
        .get(format!("{}://{}/latest-blockhash", url_prefix, base_url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let decoded_blockhash = BASE64_STANDARD.decode(resp).unwrap();
    let deserialized_blockhash = bincode::deserialize(&decoded_blockhash).unwrap();

    let ix = system_instruction::transfer(&key.pubkey(), &pool_pubkey, 1_000_000);

    let mut tx = Transaction::new_with_payer(&[ix], Some(&key.pubkey()));

    tx.sign(&[&key], deserialized_blockhash);

    let serialized_tx = bincode::serialize(&tx).unwrap();

    let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

    let resp = client
        .post(format!(
            "{}://{}/v2/signup?miner={}&fee_payer={}",
            url_prefix,
            base_url,
            miner_pubkey.to_string(),
            key.pubkey().to_string(),
        ))
        .body(encoded_tx)
        .send()
        .await;
    if let Ok(res) = resp {
        if let Ok(txt) = res.text().await {
            match txt.as_str() {
                "SUCCESS" => {
                    println!("  Successfully signed up!");
                }
                "EXISTS" => {
                    println!("  You're already signed up!");
                }
                _ => {
                    println!("  Transaction failed, please try again.");
                }
            }
        } else {
            println!("  Transaction failed, please wait and try again.");
        }
    } else {
        println!("  Transaction failed, please wait and try again.");
    }
}
