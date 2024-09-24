use std::io::Write;

use bip39::{Mnemonic, Seed};
use dirs::home_dir;
use solana_sdk::{
    derivation_path::DerivationPath, signature::{write_keypair_file, Keypair}, signer::{SeedDerivable, Signer}
};
use tokio::fs;

use crate::CONFIG_FILE;

pub async fn generate_key() {
    println!("Generating Mining Hot Wallet...");

    let new_mnemonic = Mnemonic::new(bip39::MnemonicType::Words24, bip39::Language::English);
    let phrase = new_mnemonic.clone().into_phrase();

    let seed = Seed::new(&new_mnemonic, "");

    let derivation_path = DerivationPath::from_absolute_path_str("m/44'/501'/0'/0'").unwrap();

    if let Ok(new_key) = Keypair::from_seed_and_derivation_path(seed.as_bytes(), Some(derivation_path)) {
        let dir = home_dir();

        if let Some(dir) = dir {
            let key_dir = dir.join(".config/solana/mining-hot-wallet.json");

            if key_dir.exists() {
                println!("Keypair already exists at {:?}", key_dir);
                return;
            }

            if let Some(parent_dir) = key_dir.parent() {
                if !parent_dir.exists() {
                    match fs::create_dir_all(parent_dir).await {
                        Ok(_) => {}, 
                        Err(e) => {
                            println!("Failed to create directory for wallet: {}", e);
                            return;
                        }
                    }
                }
            }

            match write_keypair_file(&new_key, key_dir.clone()) {
                Ok(_) => {
                    let config_path = std::path::PathBuf::from(CONFIG_FILE);
                    let mut file = std::fs::OpenOptions::new()
                        .append(true)
                        .open(&config_path)
                        .expect("Failed to open configuration file for appending.");

                    writeln!(file, "{}", key_dir.to_str().expect("Failed to key_dir.to_str()"))
                        .expect("Failed to write keypair path to configuration file.");

                    println!("Mining Hot Wallet Secret Phrase (Use this to import/recover your mining hot wallet): \n{}", phrase);

                    let pubkey = new_key.pubkey();
                    println!("\nNew Mining Hot Wallet Public Key: {}", pubkey);
                    println!("\nPlease send 0.002 sol and signup");
                },
                Err(e) => {
                    println!("Failed to write keypair to file: {}", e);
                }
            }
        } else {
            println!("Failed to get home directory from platform.");
        }

    } else {
        println!("Failed to generate keypair, please try again. Contact support if this keeps happening.");
    }
}
