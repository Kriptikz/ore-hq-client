use std::{str::FromStr};
use base64::{prelude::BASE64_STANDARD, Engine};
use clap::Parser;
use colored::*;
use inquire::{Text, InquireError};
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::Transaction};
use spl_associated_token_account::get_associated_token_address;

use crate::stake_balance;

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
    let url_prefix = if unsecure { "http".to_string() } else { "https".to_string() };

    // Fetch the staked balance
    let staked_balance = stake_balance::get_staked_balance(&key, base_url.clone(), unsecure).await;
    println!("  Current Staked Balance: {:.11} ORE", staked_balance);

    // Ensure unstake amount does not exceed staked balance
    let unstake_amount = if args.amount > staked_balance {
        println!("  Unstake amount exceeds staked balance. Defaulting to maximum available: {:.11} ORE", staked_balance);
        staked_balance
    } else {
        args.amount
    };

// Add confirmation step with red text before unstaking
match Text::new(
    &format!(
        "  Are you sure you want to unstake {} ORE? (Y/n or 'esc' to cancel)",
        unstake_amount
    )
    .red()
    .to_string(),
)
.prompt()
{
    Ok(confirm) => {
        if confirm.trim().eq_ignore_ascii_case("esc") {
            println!("  Unstaking canceled.");
            return;
        } else if confirm.trim().is_empty() || confirm.trim().to_lowercase() == "y" {
            // Proceed with unstaking
        } else {
            println!("  Unstaking canceled.");
            return;
        }
    }
    Err(InquireError::OperationCanceled) => {
        println!("  Unstaking operation canceled.");
        return;
    }
    Err(_) => {
        println!("  Invalid input. Unstaking canceled.");
        return;
    }
}


    // Continue with transaction
    let resp = client.get(format!("{}://{}/pool/authority/pubkey", url_prefix, base_url)).send().await.unwrap().text().await.unwrap();
    let pool_pubkey = Pubkey::from_str(&resp).unwrap();

    let resp = client.get(format!("{}://{}/pool/fee_payer/pubkey", url_prefix, base_url)).send().await.unwrap().text().await.unwrap();
    let fee_pubkey = Pubkey::from_str(&resp).unwrap();

    let resp = client.get(format!("{}://{}/latest-blockhash", url_prefix, base_url)).send().await.unwrap().text().await.unwrap();
    let decoded_blockhash = BASE64_STANDARD.decode(resp).unwrap();
    let deserialized_blockhash = bincode::deserialize(&decoded_blockhash).unwrap();

    let ata_address = get_associated_token_address(&key.pubkey(), &ore_api::consts::MINT_ADDRESS);

    let unstake_amount_u64 = (unstake_amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;
    let ix = ore_miner_delegation::instruction::undelegate_stake(key.pubkey(), pool_pubkey, ata_address, unstake_amount_u64);

    let mut tx = Transaction::new_with_payer(&[ix], Some(&fee_pubkey));
    tx.partial_sign(&[&key], deserialized_blockhash);

    let serialized_tx = bincode::serialize(&tx).unwrap();
    let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

    let resp = client.post(format!("{}://{}/unstake?pubkey={}&amount={}", url_prefix, base_url, key.pubkey().to_string(), unstake_amount_u64)).body(encoded_tx).send().await;
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