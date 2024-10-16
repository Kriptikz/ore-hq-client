use solana_sdk::signature::Keypair;
use std::error::Error;
use inquire::{Select, Confirm};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use crate::balance::{get_balance, get_token_balance};

pub async fn delegate_boost_ui(
    key: &Keypair,
    base_url: String,
    unsecure_conn: bool,
) -> Result<(), Box<dyn Error>> {
    let boost_options = vec![
        ("ORE Stake", "oreoU2P8bN6jkk3jbaiVxYnG1dCXcYxwhwyK9jSybcp"),
        ("ORE-SOL LP Token", "DrSS5RM7zUd9qjUEdDaf31vnDUSbCrMto6mjqTrHFifN"),
        ("ORE-ISC LP Token", "meUwDp23AaxhiNKaQCyJ2EAF2T4oe1gSkEkGXSRVdZb"),
    ];

    // Menu for the user
    let selection = Select::new(
        "  Select a boost option:",
        boost_options.iter().map(|(name, _)| *name).collect::<Vec<&str>>(),
    )
    .with_vim_mode(false)
    .prompt()
    .map_err(|e| {
        println!("  Selection canceled or failed: {}", e);
        e
    })?;

    // Retrieve the public key based on the user's selection
    let selected_pubkey = boost_options
        .iter()
        .find(|(name, _)| *name == selection)
        .map(|(_, pubkey)| pubkey)
        .expect("Selected option should have a corresponding pubkey.");

    println!("  You have selected: {}", selection);
    println!("  Delegating boost to Pubkey: {}", selected_pubkey);

    // Confirm the delegation action with the user
    let confirm = Confirm::new(&format!(
        "  Are you sure you want to delegate stake {} tokens?",
        selected_pubkey
    ))
    .with_default(true)
    .prompt()
    .map_err(|e| {
        println!("  Confirmation failed: {}", e);
        e
    })?;

    if !confirm {
        println!("  Delegation canceled by the user.");
        return Ok(());
    }

    // Parse the selected public key
    let pubkey = Pubkey::from_str(selected_pubkey).map_err(|e| {
        println!("  Invalid public key format: {}", e);
        e
    })?;

    println!(
        "  Delegating boosts for {} is not yet implemented.",
        pubkey
    );

    Ok(())
}
