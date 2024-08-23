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

pub async fn claim(args: ClaimArgs, key: Keypair, url: String, unsecure: bool) {
    let mut claim_amount = (args.amount * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    if claim_amount == 0 {
        println!("You entered 0 rewards to claim, so no claim will be made.");
        return;
    }

    let base_url = url;
    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    let balance = client.get(format!("{}://{}/miner/rewards?pubkey={}", url_prefix, base_url, key.pubkey().to_string())).send().await.unwrap().text().await.unwrap();
    let balance_grains = (balance.parse::<f64>().unwrap() * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64)) as u64;

    if claim_amount > balance_grains {
        println!("You do not have enough rewards to claim {} ORE.", amount_to_ui_amount(claim_amount, ore_api::consts::TOKEN_DECIMALS));
        println!("Please enter an amount less than or equal to {} ORE.", amount_to_ui_amount(balance_grains, ore_api::consts::TOKEN_DECIMALS));
        return;
    }

    println!("Sending claim request for amount {}...", amount_to_ui_amount(claim_amount, ore_api::consts::TOKEN_DECIMALS));
    let resp = client.post(format!("{}://{}/claim?pubkey={}&amount={}", url_prefix, base_url, key.pubkey().to_string(), claim_amount)).send().await;

    match resp {
        Ok(res) => {
            let response_text = res.text().await.unwrap();
            if response_text == "SUCCESS" {
                println!("Successfully claimed rewards!");
            } else if let Ok(time) = response_text.parse::<u64>() {
                let time_left = 1800 - time;
                let secs = time_left % 60;
                let mins = (time_left / 60) % 60;
                println!("Error: You cannot claim until the time is up. Time left until next claim available: {}m {}s", mins, secs);
            } else {
                println!("Unexpected response: {}", response_text);
            }
        },
        Err(e) => {
            println!("ERROR: {}", e);
            println!("Retrying in 5 seconds...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
}