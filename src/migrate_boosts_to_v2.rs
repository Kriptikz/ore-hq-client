use base64::{prelude::BASE64_STANDARD, Engine};
use solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::Signer};
use std::str::FromStr;

use crate::balance;

pub async fn migrate_boosts_to_v2(key: Keypair, url: String, unsecure: bool) {
    println!("Migrating Boosts...");
    let base_url = url;
    let client = reqwest::Client::new();
    let url_prefix = if unsecure {
        "http".to_string()
    } else {
        "https".to_string()
    };

    let token_mints = vec![
        (Pubkey::from_str("oreoU2P8bN6jkk3jbaiVxYnG1dCXcYxwhwyK9jSybcp").unwrap(), "ORE Token"),
        (Pubkey::from_str("DrSS5RM7zUd9qjUEdDaf31vnDUSbCrMto6mjqTrHFifN").unwrap(), "ORE-SOL LP"),
        (Pubkey::from_str("meUwDp23AaxhiNKaQCyJ2EAF2T4oe1gSkEkGXSRVdZb").unwrap(), "ORE-ISC LP"),
    ];

    let ore_v1_boost_amount = balance::get_boosted_stake_balance(&key, base_url.clone(), unsecure, token_mints[0].0.to_string()).await;
    let ore_sol_v1_boost_amount = balance::get_boosted_stake_balance(&key, base_url.clone(), unsecure, token_mints[1].0.to_string()).await;
    let ore_isc_v1_boost_amount = balance::get_boosted_stake_balance(&key, base_url.clone(), unsecure, token_mints[2].0.to_string()).await;

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

    if ore_v1_boost_amount > 0.0 {
        println!("Migrating {} ORE", ore_v1_boost_amount);
        // migrate ore boost
        let ore_v2_boost_amount = balance::get_boosted_stake_balance_v2(&key, base_url.clone(), unsecure, token_mints[0].0.to_string()).await;
        let mut ixs = vec![];
        // init boost account
        if ore_v2_boost_amount < 0.0 {
            // add init ix
            let ix = ore_miner_delegation::instruction::init_delegate_boost_v2(key.pubkey(), pool_pubkey, fee_pubkey, token_mints[0].0);
            ixs.push(ix);
        }
        // migrate balance
        let ix = ore_miner_delegation::instruction::migrate_boost_to_v2(key.pubkey(), pool_pubkey, token_mints[0].0);
        ixs.push(ix);
        let mut tx = solana_sdk::transaction::Transaction::new_with_payer(&ixs, Some(&fee_pubkey));
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
        tx.partial_sign(&[&key], deserialized_blockhash);
        let serialized_tx = bincode::serialize(&tx).unwrap();
        let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

        let needs_init = ixs.len() > 1;

        let resp = client
            .post(format!(
                "{}://{}/v2/migrate-boost?pubkey={}&mint={}&init={}",
                url_prefix,
                base_url,
                key.pubkey().to_string(),
                token_mints[0].0.to_string(),
                needs_init
            ))
            .body(encoded_tx)
            .send()
            .await;
        if let Ok(res) = resp {
            if let Ok(txt) = res.text().await {
                match txt.as_str() {
                    "SUCCESS" => {
                        println!("  Successfully migrated ore boost!");
                    }
                    other => {
                        println!("  Boost Migration Transaction failed: {}", other);
                    }
                }
            } else {
                println!("  Boost Migration Transaction failed, please wait and try again.");
            }
        } else {
            println!("  Boost Migration Transaction failed, please wait and try again.");
        }
    } else {
        println!("No boost v1 ORE to migrate");
    }

    if ore_sol_v1_boost_amount > 0.0 {
        println!("Migrating {} ORE-SOL", ore_sol_v1_boost_amount);
        // migrate ore boost
        let ore_v2_boost_amount = balance::get_boosted_stake_balance_v2(&key, base_url.clone(), unsecure, token_mints[1].0.to_string()).await;
        let mut ixs = vec![];
        // init boost account
        if ore_v2_boost_amount < 0.0 {
            // add init ix
            let ix = ore_miner_delegation::instruction::init_delegate_boost_v2(key.pubkey(), pool_pubkey, fee_pubkey, token_mints[1].0);
            ixs.push(ix);
        }
        // migrate balance
        let ix = ore_miner_delegation::instruction::migrate_boost_to_v2(key.pubkey(), pool_pubkey, token_mints[1].0);
        ixs.push(ix);
        let mut tx = solana_sdk::transaction::Transaction::new_with_payer(&ixs, Some(&fee_pubkey));
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
        tx.partial_sign(&[&key], deserialized_blockhash);
        let serialized_tx = bincode::serialize(&tx).unwrap();
        let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

        let needs_init = ixs.len() > 1;

        let resp = client
            .post(format!(
                "{}://{}/v2/migrate-boost?pubkey={}&mint={}&init={}",
                url_prefix,
                base_url,
                key.pubkey().to_string(),
                token_mints[1].0.to_string(),
                needs_init
            ))
            .body(encoded_tx)
            .send()
            .await;
        if let Ok(res) = resp {
            if let Ok(txt) = res.text().await {
                match txt.as_str() {
                    "SUCCESS" => {
                        println!("  Successfully migrated ore-sol boost!");
                    }
                    other => {
                        println!("  Boost Migration Transaction failed: {}", other);
                    }
                }
            } else {
                println!("  Boost Migration Transaction failed, please wait and try again.");
            }
        } else {
            println!("  Boost Migration Transaction failed, please wait and try again.");
        }
    } else {
        println!("No boost v1 ORE-SOL to migrate");
    }

    if ore_isc_v1_boost_amount > 0.0 {
        println!("Migrating {} ORE-ISC", ore_isc_v1_boost_amount);
        // migrate ore boost
        let ore_v2_boost_amount = balance::get_boosted_stake_balance_v2(&key, base_url.clone(), unsecure, token_mints[2].0.to_string()).await;
        let mut ixs = vec![];
        // init boost account
        if ore_v2_boost_amount < 0.0 {
            // add init ix
            let ix = ore_miner_delegation::instruction::init_delegate_boost_v2(key.pubkey(), pool_pubkey, fee_pubkey, token_mints[2].0);
            ixs.push(ix);
        }
        // migrate balance
        let ix = ore_miner_delegation::instruction::migrate_boost_to_v2(key.pubkey(), pool_pubkey, token_mints[2].0);
        ixs.push(ix);
        let mut tx = solana_sdk::transaction::Transaction::new_with_payer(&ixs, Some(&fee_pubkey));
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
        tx.partial_sign(&[&key], deserialized_blockhash);
        let serialized_tx = bincode::serialize(&tx).unwrap();
        let encoded_tx = BASE64_STANDARD.encode(&serialized_tx);

        let needs_init = ixs.len() > 1;

        let resp = client
            .post(format!(
                "{}://{}/v2/migrate-boost?pubkey={}&mint={}&init={}",
                url_prefix,
                base_url,
                key.pubkey().to_string(),
                token_mints[2].0.to_string(),
                needs_init
            ))
            .body(encoded_tx)
            .send()
            .await;
        if let Ok(res) = resp {
            if let Ok(txt) = res.text().await {
                match txt.as_str() {
                    "SUCCESS" => {
                        println!("  Successfully migrated ore-isc boost!");
                    }
                    other => {
                        println!("  Boost Migration Transaction failed: {}", other);
                    }
                }
            } else {
                println!("  Boost Migration Transaction failed, please wait and try again.");
            }
        } else {
            println!("  Boost Migration Transaction failed, please wait and try again.");
        }
    } else {
        println!("No boost v1 ORE-ISC to migrate");
    }

    println!("Boost Migrations Complete");
}
