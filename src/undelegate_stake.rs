use std::{str::FromStr, time::Duration};

use base64::{prelude::BASE64_STANDARD, Engine};
use clap::Parser;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::Transaction};
use spl_associated_token_account::get_associated_token_address;

#[derive(Debug, Parser)]
pub struct UnstakeArgs {
    #[arg(
        long,
        value_name = "AMOUNT",
        help = "Amount of ore to unstake."
    )]
    pub amount: f64,
}


pub async fn undelegate_stake(args: UnstakeArgs, key: &Keypair, url: String, unsecure: bool) {
    let base_url = url;

    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    let resp = client.get(format!("{}://{}/pool/authority/pubkey", url_prefix, base_url)).send().await.unwrap().text().await.unwrap();

    let pool_pubkey = Pubkey::from_str(&resp).unwrap();

    let resp = client.get(format!("{}://{}/pool/fee_payer/pubkey", url_prefix, base_url)).send().await.unwrap().text().await.unwrap();

    let fee_pubkey = Pubkey::from_str(&resp).unwrap();

    let resp = client.get(format!("{}://{}/latest-blockhash", url_prefix, base_url)).send().await.unwrap().text().await.unwrap();

    let decoded_blockhash = BASE64_STANDARD.decode(resp).unwrap();
    let deserialized_blockhash = bincode::deserialize(&decoded_blockhash).unwrap();

    let ata_address = get_associated_token_address(&key.pubkey(), &ore_api::consts::MINT_ADDRESS);

    let stake_amount = (args.amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;
    let ix = ore_miner_delegation::instruction::undelegate_stake(key.pubkey(), pool_pubkey, ata_address, stake_amount);

    let mut tx = Transaction::new_with_payer(&[ix], Some(&fee_pubkey));

    tx.partial_sign(&[&key], deserialized_blockhash);

    let serialized_tx = bincode::serialize(&tx).unwrap();

    let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

    let resp = client.post(format!("{}://{}/unstake?pubkey={}&amount={}", url_prefix, base_url, key.pubkey().to_string(), stake_amount)).body(encoded_tx).send().await;
    if let Ok(res) = resp {
        if let Ok(txt) = res.text().await {
            match txt.as_str() {
                "SUCCESS" => {
                    println!("  Successfully unstaked!");
                },
                other => {
                    println!("  Transaction failed: {}", other);
                }
            }
        } else {
            println!("  Transaction failed, please wait and try again.");
        }
    } else {
        println!("  Transaction failed, please wait and try again.");
    }
}