use inquire::{Text, InquireError};
use std::time::Duration;
use std::io::{self, Write};
use clap::Parser;
use solana_sdk::{signature::Keypair, signer::Signer};
use colored::*;
use spl_token::amount_to_ui_amount;
use std::thread::sleep;
use std::io::Read;

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

    // Fetch and display balance and rewards
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

    let balance = balance_response.parse::<f64>().unwrap_or(0.0);

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

    println!();
    println!("  Unclaimed Rewards: {:.11} ORE", rewards);
    println!("  Wallet Balance:    {:.11} ORE", balance);

    // Convert balance to grains
    let balance_grains = (rewards * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    // If balance is zero, inform the user and return to keypair selection
    if balance_grains == 0 {
        println!("\n  There is no balance to claim.");
        prompt_to_continue(); // Pause before returning
        return;
    }

    let mut claim_amount = args.amount.unwrap_or(0.0);

    // Prompt the user for an amount if it's not provided or less than 0.005
    loop {
        if claim_amount < 0.005 {
            if claim_amount != 0.0 { // Only show the message if they previously entered an invalid value
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
    let claim_amount_grains = (claim_amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    // Handle the case where the claim amount is zero
    if claim_amount_grains == 0 {
        println!("  You entered 0 rewards to claim, so no claim will be made.");
        prompt_to_continue();
        return;
    }

    // Ensure the claim amount does not exceed the available balance
    if claim_amount_grains > balance_grains {
        println!(
            "  You do not have enough rewards to claim {} ORE.",
            amount_to_ui_amount(claim_amount_grains, ore_api::consts::TOKEN_DECIMALS)
        );
        println!(
            "  Please enter an amount less than or equal to {} ORE.",
            amount_to_ui_amount(balance_grains, ore_api::consts::TOKEN_DECIMALS)
        );
        prompt_to_continue();
        return;
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
            } else if confirm.trim().to_lowercase() != "y" {
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

    prompt_to_continue(); // Pause after the claim operation completes
}

fn prompt_to_continue() {
    sleep(Duration::from_millis(100));
    println!("\n  Press any key to continue...");
    let _ = io::stdin().read(&mut [0u8]).unwrap();
}
