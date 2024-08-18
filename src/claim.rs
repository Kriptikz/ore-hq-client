use std::time::Duration;
use std::io::{self, Write};
use spl_token::amount_to_ui_amount;

use clap::Parser;
use solana_sdk::{signature::Keypair, signer::Signer};

#[derive(Debug, Parser)]
pub struct ClaimArgs {
    #[arg(
        long,
        value_name = "AMOUNT",
        default_value = "0.00",
        help = "Amount of ore to claim."
    )]
    pub amount: f64,
}

pub fn ask_confirm(question: &str) -> bool {
    println!("{}", question);
    loop {
        io::stdout().flush().unwrap();

        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);

        match input.trim().chars().next() {
            Some('y') | Some('Y') => return true,
            Some('n') | Some('N') => return false,
            _ => println!("Please type only Y or N to continue."),
        }
    }
}

pub async fn claim(args: ClaimArgs, key: Keypair, url: String, unsecure: bool) {
    let mut claim_amount = (args.amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    loop {
        let balance = client.get(format!("{}://{}/miner/rewards?pubkey={}", url_prefix, base_url, key.pubkey().to_string())).send().await.unwrap().text().await.unwrap();
        let balance_grains = (balance.parse::<f64>().unwrap() * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;
        println!("Claimable Rewards: {} ORE", balance);

        if claim_amount == 0 {
            claim_amount = balance_grains;
        }

        if claim_amount > balance_grains {
            println!("You do not have enough rewards to claim {} ORE.", amount_to_ui_amount(claim_amount, ore_api::consts::TOKEN_DECIMALS));
            println!("Please enter an amount less than or equal to {} ORE.", amount_to_ui_amount(balance_grains, ore_api::consts::TOKEN_DECIMALS));
            claim_amount = loop {
                let mut input = String::new();
                io::stdout().flush().unwrap();
                let _ = std::io::stdin().read_line(&mut input);
                if let Ok(new_amount) = input.trim().parse::<f64>() {
                    let new_claim_amount = (new_amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;
                    if new_claim_amount <= balance_grains {
                        break new_claim_amount;
                    }
                }
                println!("Invalid input. Please enter a valid amount.");
            };
        }

        // Confirm user wants to claim
        if !ask_confirm(
            format!(
                "\nYou are about to claim {}.\nAre you sure you want to continue? [Y/n]",
                format!(
                    "{} ORE",
                    amount_to_ui_amount(claim_amount, ore_api::consts::TOKEN_DECIMALS)
                )
            )
            .as_str(),
        ) {
            return;
        }

        println!("Sending claim request for amount {}...", claim_amount);
        let resp = client.post(format!("{}://{}/claim?pubkey={}&amount={}", url_prefix, base_url, key.pubkey().to_string(), claim_amount)).send().await;

        match resp {
            Ok(res) => {
                match res.text().await.unwrap().as_str() {
                    "SUCCESS" => {
                        println!("Successfully claimed rewards!");
                        break;
                    },
                    other => {
                        let time = other.parse::<u64>().unwrap();
                        let time_left = 1800 - time;
                        let secs = time_left % 60;
                        let mins = (time_left / 60) % 60;
                        println!("Time left until next claim available: {}m {}s", mins, secs);
                        break;
                    }
                }

            },
            Err(e) => {
                println!("ERROR: {}", e);
                println!("Retrying in 5 seconds...");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
