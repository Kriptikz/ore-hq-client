use base64::{prelude::BASE64_STANDARD, Engine};
use clap::Parser;
use colored::*;
use inquire::{InquireError, Text};
use reqwest::StatusCode;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer, transaction::Transaction};
use std::{str::FromStr, time::Duration};

use crate::balance::get_token_balance;

#[derive(Debug, Parser)]
pub struct BoostArgs {
    #[arg(long, value_name = "AMOUNT", help = "Amount of ore to stake.")]
    pub amount: f64,

    #[arg(long, value_name = "MINT", help = "Mint of boost.")]
    pub mint: String,

    #[arg(
        long,
        short,
        action,
        help = "Auto stake input amount when staking window opens."
    )]
    pub auto: bool,
}

pub async fn delegate_boost(args: BoostArgs, key: Keypair, url: String, unsecure: bool) {
    let base_url = url;
    let client = reqwest::Client::new();
    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };
    let balance = get_token_balance(&key, base_url.clone(), unsecure, args.mint.clone()).await;

    // Ensure stake amount does not exceed balance
    let boost_amount = if args.amount > balance {
        println!(
            "  You do not have enough to stake {} boost tokens.\n  Adjusting stake amount to the maximum available: {} boost tokens",
            args.amount, balance
        );
        balance
    } else {
        args.amount
    };

    // RED TEXT
    match Text::new(
        &format!(
            "  Are you sure you want to stake {} boost tokens? (Y/n or 'esc' to cancel)",
            boost_amount
        )
        .red()
        .to_string(),
    )
    .prompt()
    {
        Ok(confirm) => {
            if confirm.trim().eq_ignore_ascii_case("esc") {
                println!("  Boosting canceled.");
                return;
            } else if confirm.trim().is_empty() || confirm.trim().to_lowercase() == "y" {
                // Proceed with staking
            } else {
                println!("  Boosting canceled.");
                return;
            }
        }
        Err(InquireError::OperationCanceled) => {
            println!("  Boosting operation canceled.");
            return;
        }
        Err(_) => {
            println!("  Invalid input. Boosting canceled.");
            return;
        }
    }

    if !args.auto {
        // Non-auto staking logic
        let timestamp = get_timestamp(&client, &url_prefix, &base_url).await;
        println!("  Server Timestamp: {}", timestamp);
        if let Some(secs_passed_hour) = timestamp.checked_rem(3600) {
            println!("  SECS PASSED HOUR: {}", secs_passed_hour);
            if secs_passed_hour < 300 {
                println!("  Staking window opened. Staking...");
            } else {
                println!("  Staking window not currently open. Please use --auto or wait until the start of the next hour.");
                return;
            }
        } else {
            println!("  Timestamp checked_rem error. Please try again.");
            return;
        }
    } else {
        // Auto staking logic with retry mechanism
        loop {
            let timestamp = get_timestamp(&client, &url_prefix, &base_url).await;
            println!("  Server Timestamp: {}", timestamp);
            if let Some(secs_passed_hour) = timestamp.checked_rem(3600) {
                if secs_passed_hour < 300 {
                    println!("  Staking window opened. Staking...");

                    // Attempt staking transaction
                    loop {
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
                            .get(format!(
                                "{}://{}/pool/fee_payer/pubkey",
                                url_prefix, base_url
                            ))
                            .send()
                            .await
                            .unwrap()
                            .text()
                            .await
                            .unwrap();
                        let fee_pubkey = Pubkey::from_str(&resp).unwrap();

                        let resp = client
                            .get(format!("{}://{}/latest-blockhash", url_prefix, base_url))
                            .send()
                            .await
                            .unwrap()
                            .text()
                            .await
                            .unwrap();
                        let decoded_blockhash = BASE64_STANDARD.decode(resp).unwrap();
                        let deserialized_blockhash =
                            bincode::deserialize(&decoded_blockhash).unwrap();

                        let boost_amount_u64 = (boost_amount
                            * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64))
                            as u64;
                        let ix = ore_miner_delegation::instruction::delegate_boost(
                            key.pubkey(),
                            pool_pubkey,
                            Pubkey::from_str(&args.mint).unwrap(),
                            boost_amount_u64,
                        );

                        let mut tx = Transaction::new_with_payer(&[ix], Some(&fee_pubkey));
                        tx.partial_sign(&[&key], deserialized_blockhash);
                        let serialized_tx = bincode::serialize(&tx).unwrap();
                        let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

                        let resp = client
                            .post(format!(
                                "{}://{}/stake-boost?pubkey={}&mint={}&amount={}",
                                url_prefix,
                                base_url,
                                key.pubkey().to_string(),
                                args.mint,
                                boost_amount_u64
                            ))
                            .body(encoded_tx)
                            .send()
                            .await;

                        if let Ok(res) = resp {
                            if let Ok(txt) = res.text().await {
                                match txt.as_str() {
                                    "SUCCESS" => {
                                        println!("  Successfully boosted!");
                                        return; // Exit the loop and function when successful
                                    }
                                    other => {
                                        println!("  Transaction failed: {}", other);
                                    }
                                }
                            } else {
                                println!("  Transaction failed, retrying...");
                            }
                        } else {
                            println!("  Transaction failed, retrying...");
                        }

                        // Wait before trying again
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                } else {
                    println!("  Waiting for staking window to open... You can let this run until it is complete.");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            } else {
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }
    }

    // Non-auto and auto logic converge for transaction execution
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
        .get(format!(
            "{}://{}/pool/fee_payer/pubkey",
            url_prefix, base_url
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let fee_pubkey = Pubkey::from_str(&resp).unwrap();

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

    let boost_amount_u64 =
        (boost_amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;
    let ix = ore_miner_delegation::instruction::delegate_boost(
        key.pubkey(),
        pool_pubkey,
        Pubkey::from_str(&args.mint).unwrap(),
        boost_amount_u64,
    );

    let mut tx = Transaction::new_with_payer(&[ix], Some(&fee_pubkey));
    tx.partial_sign(&[&key], deserialized_blockhash);
    let serialized_tx = bincode::serialize(&tx).unwrap();
    let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

    let resp = client
        .post(format!(
            "{}://{}/stake-boost?pubkey={}&mint={}&amount={}",
            url_prefix,
            base_url,
            key.pubkey().to_string(),
            args.mint,
            boost_amount_u64
        ))
        .body(encoded_tx)
        .send()
        .await;
    if let Ok(res) = resp {
        if let Ok(txt) = res.text().await {
            match txt.as_str() {
                "SUCCESS" => {
                    println!("  Successfully boosted!");
                }
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

// Helper function to fetch server timestamp
async fn get_timestamp(client: &reqwest::Client, url_prefix: &str, base_url: &str) -> u64 {
    loop {
        if let Ok(response) = client
            .get(format!("{}://{}/timestamp", url_prefix, base_url))
            .send()
            .await
        {
            match response.status() {
                StatusCode::OK => {
                    if let Ok(ts) = response.text().await {
                        if let Ok(ts) = ts.parse::<u64>() {
                            return ts;
                        }
                    }
                }
                _ => {
                    println!("  Server restarting, trying again in 3 seconds...");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
            }
        }
        println!("  Unable to retrieve timestamp, retrying in 3 seconds...");
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}
