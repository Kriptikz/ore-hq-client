use std::{fs, io::Write};

use bip39::{Mnemonic, Seed};
use dirs::home_dir;
use qrcode::render::unicode;
use qrcode::QrCode;
use solana_sdk::{
    derivation_path::DerivationPath,
    signature::{write_keypair_file, Keypair},
    signer::{SeedDerivable, Signer},
};

use crate::CONFIG_FILE;

pub fn generate_key() {
    let new_mnemonic = Mnemonic::new(bip39::MnemonicType::Words12, bip39::Language::English);
    let phrase = new_mnemonic.clone().into_phrase();

    let seed = Seed::new(&new_mnemonic, "");

    let derivation_path = DerivationPath::from_absolute_path_str("m/44'/501'/0'/0'").unwrap();

    if let Ok(new_key) =
        Keypair::from_seed_and_derivation_path(seed.as_bytes(), Some(derivation_path))
    {
        let dir = home_dir();

        if let Some(dir) = dir {
            let key_dir = dir.join(".config/solana/mining-hot-wallet.json");

            if key_dir.exists() {
                println!("  Keypair already exists at {:?}", key_dir);
                return;
            }

            if let Some(parent_dir) = key_dir.parent() {
                if !parent_dir.exists() {
                    match fs::create_dir_all(parent_dir) {
                        Ok(_) => {}
                        Err(e) => {
                            println!("  Failed to create directory for wallet: {}", e);
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

                    writeln!(
                        file,
                        "{}",
                        key_dir.to_str().expect("Failed to key_dir.to_str()")
                    )
                    .expect("Failed to write keypair path to configuration file.");

                    let pubkey = new_key.pubkey();

                    // Generate QR code for the public key
                    if let Ok(code) = QrCode::new(pubkey.to_string()) {
                        // Render the QR code without extra indentation
                        let string = code
                            .render::<unicode::Dense1x2>()
                            .quiet_zone(false) // Remove additional padding or quiet zone
                            .build();

                        // Print the QR code with clear separators
                        println!("  QR Code for Public Key:\n");
                        println!("{}", string);
                    } else {
                        println!("  Failed to generate QR code for the public key.");
                    }

                    // Print the mining wallet information and instructions after the QR code
                    println!("  Mining Hot Wallet Secret Phrase (Use this to import/recover your mining hot wallet):");
                    println!("    {}", phrase);
                    println!("\n  New Mining Hot Wallet Public Key: {}", pubkey);
                    println!("\n  The QR code above can be scanned with Phantom/Solflare wallet to fund this wallet for any reason.");
                    println!("\n  Note: Ec1ipse Pool does not require a sign up fee.");
                }
                Err(e) => {
                    println!("  Failed to write keypair to file: {}", e);
                }
            }
        } else {
            println!("  Failed to get home directory from platform.");
        }
    } else {
        println!("  Failed to generate keypair, please try again. Contact support if this keeps happening.");
    }
}
