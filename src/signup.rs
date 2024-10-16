use std::str::FromStr;

use clap::Parser;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};

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
            Ok(pk) => pk,
            Err(_e) => {
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
        .post(format!(
            "{}://{}/v2/signup?miner={}",
            url_prefix,
            base_url,
            miner_pubkey.to_string(),
        ))
        .body("BLANK".to_string())
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
