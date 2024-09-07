use clap::Parser;
use colored::*;
use inquire::{InquireError, Text};
use solana_sdk::{signature::Keypair, signer::Signer};
use spl_token::amount_to_ui_amount;
use std::time::Duration;

#[derive(Debug, Parser)]
pub struct ClaimArgs {
    #[arg(
        long,
        value_name = "AMOUNT",
        help = "Amount of ore to claim. (Minimum of 0.005 ORE)"
    )]
    pub amount: Option<f64>,
}

pub async fn claim(args: ClaimArgs, key: Keypair, url: String, unsecure: bool) {
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
            url,
            key.pubkey().to_string()
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    let balance = if let Ok(parsed_balance) = balance_response.parse::<f64>() {
        parsed_balance
    } else {
        // If the wallet balance failed to parse
        println!("\n  Note: A 0.004 ORE fee will be deducted from your claim amount to cover the cost\n  of Token Account Creation. This is a one time fee used to create the ORE Token Account.");
        0.0
    };

    let rewards_response = client
        .get(format!(
            "{}://{}/miner/rewards?pubkey={}",
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

    let rewards = rewards_response.parse::<f64>().unwrap_or(0.0);

    println!("  Unclaimed Rewards: {:.11} ORE", rewards);
    println!("  Wallet Balance:    {:.11} ORE", balance);

    if rewards < 0.005 {
        println!();
        println!("  You have not reached the required claim limit of 0.005 ORE.");
        println!("  Keep mining to accumulate more rewards before you can withdraw.");
        return;
    }

    // Convert balance to grains
    let balance_grains = (rewards * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    // If balance is zero, inform the user and return to keypair selection
    if balance_grains == 0 {
        println!("\n  There is no balance to claim.");
        return;
    }

    let mut claim_amount = args.amount.unwrap_or(0.0);

    // Prompt the user for an amount if it's not provided or less than 0.005
    loop {
        if claim_amount < 0.005 {
            if claim_amount != 0.0 {
                // Only show the message if they previously entered an invalid value
                println!("  Please enter a number above 0.005.");
            }

            match Text::new("\n  Enter the amount to claim (minimum 0.005 ORE or 'esc' to cancel):")
                .prompt()
            {
                Ok(input) => {
                    if input.trim().eq_ignore_ascii_case("esc") {
                        println!("  Claim operation canceled.");
                        return;
                    }

                    claim_amount = match input.trim().parse::<f64>() {
                        Ok(val) if val >= 0.005 => val,
                        _ => {
                            println!("  Please enter a valid number above 0.005.");
                            continue;
                        }
                    };
                }
                Err(InquireError::OperationCanceled) => {
                    println!("  Claim operation canceled.");
                    return;
                }
                Err(_) => {
                    println!("  Invalid input. Please try again.");
                    continue;
                }
            }
        } else {
            break;
        }
    }

    // Convert the claim amount to the smallest unit
    let mut claim_amount_grains =
        (claim_amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    // Auto-adjust the claim amount if it exceeds the available balance
    if claim_amount_grains > balance_grains {
        println!(
            "  You do not have enough rewards to claim {} ORE.",
            amount_to_ui_amount(claim_amount_grains, ore_api::consts::TOKEN_DECIMALS)
        );
        claim_amount_grains = balance_grains;
        println!(
            "  Adjusting claim amount to the maximum available: {} ORE.",
            amount_to_ui_amount(claim_amount_grains, ore_api::consts::TOKEN_DECIMALS)
        );
    }

    // RED TEXT
    match Text::new(
        &format!(
            "  Are you sure you want to claim {} ORE? (Y/n or 'esc' to cancel)",
            amount_to_ui_amount(claim_amount_grains, ore_api::consts::TOKEN_DECIMALS)
        )
        .red()
        .to_string(),
    )
    .prompt()
    {
        Ok(confirm) => {
            if confirm.trim().eq_ignore_ascii_case("esc") {
                println!("  Claim canceled.");
                return;
            } else if confirm.trim().is_empty() || confirm.trim().to_lowercase() == "y" {
            } else {
                println!("  Claim canceled.");
                return;
            }
        }
        Err(InquireError::OperationCanceled) => {
            println!("  Claim operation canceled.");
            return;
        }
        Err(_) => {
            println!("  Invalid input. Claim canceled.");
            return;
        }
    }

    println!(
        "  Sending claim request for {} ORE...",
        amount_to_ui_amount(claim_amount_grains, ore_api::consts::TOKEN_DECIMALS)
    );

    let resp = client
        .post(format!(
            "{}://{}/claim?pubkey={}&amount={}",
            url_prefix,
            url,
            key.pubkey().to_string(),
            claim_amount_grains
        ))
        .send()
        .await;

    match resp {
        Ok(res) => match res.text().await.unwrap().as_str() {
            "SUCCESS" => {
                println!("  Successfully claimed rewards!");
            }
            "QUEUED" => {
                println!("  Claim is already queued for processing.");
            }
            other => {
                if let Ok(time) = other.parse::<u64>() {
                    let time_left = 1800 - time;
                    let secs = time_left % 60;
                    let mins = (time_left / 60) % 60;
                    println!(
                        "  You cannot claim until the time is up. Time left until next claim available: {}m {}s",
                        mins, secs
                    );
                } else {
                    println!("  Unexpected response: {}", other);
                }
            }
        },
        Err(e) => {
            println!("  ERROR: {}", e);
            println!("  Retrying in 5 seconds...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}
