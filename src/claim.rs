use std::time::Duration;
use std::io::{self, Write, Read};
use spl_token::amount_to_ui_amount;
use clap::Parser;
use solana_sdk::{signature::Keypair, signer::Signer};
use colored::*;
use std::thread::sleep;

#[derive(Debug, Parser)]
pub struct ClaimArgs {
    #[arg(
        long,
        value_name = "AMOUNT",
        help = "Amount of ore to claim."
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

    let balance = match balance_response.parse::<f64>() {
        Ok(b) => b,
        Err(_) => 0.0,
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

    let rewards = match rewards_response.parse::<f64>() {
        Ok(r) => r,
        Err(_) => 0.0,
    };

    println!();
    println!("Unclaimed Rewards: {:.11} ORE", rewards);
    println!("Wallet Balance:    {:.11} ORE", balance);

    // Convert balance to grains
    let balance_grains = (rewards * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    // If balance is zero, inform the user and return to keypair selection
    if balance_grains == 0 {
        println!("\nThere is no balance to claim.");
        prompt_to_continue(); // Pause before returning
        return;
    }

    // Prompt for amount if not provided
    let claim_amount = if let Some(amount) = args.amount {
        amount
    } else {
        print!("\nEnter the amount to claim: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        match input.trim().parse::<f64>() {
            Ok(val) => val,
            Err(_) => {
                println!("Please enter a valid number.");
                prompt_to_continue(); // Pause before returning
                return;
            }
        }
    };

    // Convert the claim amount to the smallest unit
    let claim_amount_grains = (claim_amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    // Handle the case where the claim amount is zero
    if claim_amount_grains == 0 {
        println!("You entered 0 rewards to claim, so no claim will be made.");
        prompt_to_continue(); // Pause before returning
        return;
    }

    // Ensure the claim amount does not exceed the available balance
    if claim_amount_grains > balance_grains {
        println!(
            "You do not have enough rewards to claim {} ORE.",
            amount_to_ui_amount(claim_amount_grains, ore_api::consts::TOKEN_DECIMALS)
        );
        println!(
            "Please enter an amount less than or equal to {} ORE.",
            amount_to_ui_amount(balance_grains, ore_api::consts::TOKEN_DECIMALS)
        );
        prompt_to_continue(); // Pause before returning
        return;
    }

    // Ask for confirmation with red colored text
    println!(
        "{}",
        format!(
            "Are you sure you want to claim {} ORE? (Y/n)",
            amount_to_ui_amount(claim_amount_grains, ore_api::consts::TOKEN_DECIMALS)
        )
        .red()
    );
    io::stdout().flush().unwrap();

    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm).unwrap();

    let confirm = confirm.trim().to_lowercase();
    if confirm != "y" && !confirm.is_empty() && confirm != "yes" {
        println!("Claim cancelled.");
        prompt_to_continue(); // Pause before returning
        return;
    }

    println!(
        "Sending claim request for amount {}...",
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
        Ok(res) => {
            let response_text = res.text().await.unwrap();
            if (response_text == "SUCCESS") {
                println!("Successfully claimed rewards!");
            } else if let Ok(time) = response_text.parse::<u64>() {
                let time_left = 1800 - time;
                let secs = time_left % 60;
                let mins = (time_left / 60) % 60;
                println!(
                    "Error: You cannot claim until the time is up. Time left until next claim available: {}m {}s",
                    mins, secs
                );
            } else {
                println!("Unexpected response: {}", response_text);
            }
        }
        Err(e) => {
            println!("ERROR: {}", e);
            println!("Retrying in 5 seconds...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    prompt_to_continue(); // Pause after the claim operation completes
}

fn prompt_to_continue() {
    sleep(Duration::from_millis(100));
    println!("\nPress any key to continue...");
    let _ = io::stdin().read(&mut [0u8]).unwrap();
}
