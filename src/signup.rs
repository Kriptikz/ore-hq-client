use std::str::FromStr;
use std::io::Read;
use spl_token::amount_to_ui_amount;

use base64::{prelude::BASE64_STANDARD, Engine};
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer, system_instruction, transaction::Transaction};


pub fn ask_confirm(question: &str) -> bool {
    println!("{}", question);
    loop {
        let mut input = [0];
        let _ = std::io::stdin().read(&mut input);
        match input[0] as char {
            'y' | 'Y' => return true,
            'n' | 'N' => return false,
            _ => println!("Please type only Y or N to continue."),
        }
    }
}


pub async fn signup(url: String, key: Keypair, unsecure: bool) {
    let base_url = url;

    if !ask_confirm(
        format!(
            "\nYou are about to sign up to mine with Ec1ipse Mining Pool, this costs 0.001 Solana.\nWould you like to continue? [Y/n]"
        )
        .as_str(),
    ) {
        return;
    }

    let client = reqwest::Client::new();

    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    let resp = client.get(format!("{}://{}/pool/authority/pubkey", url_prefix, base_url)).send().await.unwrap().text().await.unwrap();

    let pool_pubkey = Pubkey::from_str(&resp).unwrap();

    let resp = client.get(format!("{}://{}/latest-blockhash", url_prefix, base_url)).send().await.unwrap().text().await.unwrap();

    let decoded_blockhash = BASE64_STANDARD.decode(resp).unwrap();
    let deserialized_blockhash = bincode::deserialize(&decoded_blockhash).unwrap();

    let ix = system_instruction::transfer(&key.pubkey(), &pool_pubkey, 1_000_000);

    let mut tx = Transaction::new_with_payer(&[ix], Some(&key.pubkey()));

    tx.sign(&[&key], deserialized_blockhash);

    let serialized_tx = bincode::serialize(&tx).unwrap();

    let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

    let resp = client.post(format!("{}://{}/signup?pubkey={}", url_prefix, base_url, key.pubkey().to_string())).body(encoded_tx).send().await;
    if let Ok(res) = resp {
        if let Ok(txt) = res.text().await {
            match txt.as_str() {
                "SUCCESS" => {
                    println!("Successfully signed up!");
                },
                _ => {
                    println!("Transaction failed, please wait and try again.");
                }
            }
        } else {
            println!("Transaction failed, please wait and try again.");
        }
    } else {
        println!("Transaction failed, please wait and try again.");
    }
}
