use std::{
    ops::Range,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::prelude::*;
use clap::{arg, Parser};
use futures_util::{SinkExt, StreamExt};
use rayon::prelude::*;
use solana_sdk::{signature::Keypair, signer::Signer};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        handshake::client::{generate_key, Request},
        Message,
    },
};
use std::sync::Once;
use std::sync::atomic::AtomicU64;

static INIT_RAYON: Once = Once::new();

// Constants for tuning performance
const MIN_CHUNK_SIZE: u64 = 3_000_000;
const MAX_CHUNK_SIZE: u64 = 30_000_000;

#[derive(Debug)]
pub enum ServerMessage {
    StartMining([u8; 32], Range<u64>, u64),
}

#[derive(Debug, Parser)]
pub struct MineArgs {
    #[arg(
        long,
        value_name = "CORES",
        default_value = "1",
        help = "Number of cores to use while mining"
    )]
    pub cores: usize,

    #[arg(
        long,
        short,
        value_name = "EXPECTED_MIN_DIFFICULTY",
        help = "The expected min difficulty to submit for miner.",
        default_value = "17"
    )]
    pub expected_min_difficulty: u32,
}

struct MiningResult {
    nonce: u64,
    difficulty: u32,
    hash: drillx_2::Hash,
    nonces_checked: u64,
}

impl MiningResult {
    fn new() -> Self {
        MiningResult {
            nonce: 0,
            difficulty: 0,
            hash: drillx_2::Hash::default(),  // Assuming drillx::Hash implements Default
            nonces_checked: 0,
        }
    }
}

fn calculate_dynamic_chunk_size(nonce_range: &Range<u64>, threads: usize) -> u64 {
    let range_size = nonce_range.end - nonce_range.start;
    let chunks_per_thread = 5;
    let ideal_chunk_size = range_size / (threads * chunks_per_thread) as u64;
    
    ideal_chunk_size.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE)
}

fn optimized_mining_rayon(
    challenge: &[u8; 32],
    nonce_range: Range<u64>,
    cutoff_time: u64,
    global_best_difficulty: &AtomicU32,
    adaptive_min_difficulty: &AtomicU32,
    cores: usize,
) -> (u64, u32, drillx_2::Hash, u64) {
    let stop_signal = Arc::new(AtomicBool::new(false));
    let total_nonces_checked = Arc::new(AtomicU64::new(0));
    
    // Initialize Rayon thread pool only once
    INIT_RAYON.call_once(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(cores)
            .build_global()
            .expect("Failed to initialize global thread pool");
    });
    
    let chunk_size = calculate_dynamic_chunk_size(&nonce_range, cores);
    let start_time = Instant::now();
    
    let results: Vec<MiningResult> = (0..cores).into_par_iter().map(|core_id| {
        let core_range_size = (nonce_range.end - nonce_range.start) / cores as u64;
        let core_start = nonce_range.start + core_id as u64 * core_range_size;
        let core_end = if core_id == cores - 1 { nonce_range.end } else { core_start + core_range_size };
        
        let mut core_best = MiningResult::new();
        let mut local_nonces_checked = 0;

        'outer: for chunk_start in (core_start..core_end).step_by(chunk_size as usize) {
            let chunk_end = (chunk_start + chunk_size).min(core_end);
            for nonce in chunk_start..chunk_end {
                if start_time.elapsed().as_secs() >= cutoff_time {
                    break 'outer;
                }

                if stop_signal.load(Ordering::Relaxed) {
                    break 'outer;
                }

                local_nonces_checked += 1;
                
                if let Ok(hx) = drillx_2::hash(challenge, &nonce.to_le_bytes()) {
                    let difficulty = hx.difficulty();
                    
                    if difficulty > core_best.difficulty {
                        core_best = MiningResult {
                            nonce,
                            difficulty,
                            hash: hx,
                            nonces_checked: local_nonces_checked,
                        };
                        let _ = global_best_difficulty.fetch_max(difficulty, Ordering::Release);
                        let _ = adaptive_min_difficulty.fetch_max(difficulty.saturating_sub(2), Ordering::Relaxed);
                    }
                }
            }
        }
        
        total_nonces_checked.fetch_add(local_nonces_checked, Ordering::Relaxed);
        core_best
    }).collect();

    stop_signal.store(true, Ordering::Relaxed);

    let best_result = results.into_iter()
        .reduce(|acc, x| {
            if x.difficulty > acc.difficulty {
                x
            } else {
                acc
            }
        })
        .unwrap_or_else(MiningResult::new);

    (best_result.nonce, best_result.difficulty, best_result.hash, total_nonces_checked.load(Ordering::Relaxed))
}

pub async fn mine(args: MineArgs, key: Keypair, url: String, unsecure: bool) {
    let mut cores = args.cores;
    let max_cores = core_affinity::get_core_ids().unwrap().len();
    if cores > max_cores {
        cores = max_cores
    }

    loop {
        let base_url = url.clone();
        let mut ws_url_str = if unsecure {
            format!("ws://{}", url)
        } else {
            format!("wss://{}", url)
        };

        if !ws_url_str.ends_with('/') {
            ws_url_str.push('/');
        }

        let client = reqwest::Client::new();

        let http_prefix = if unsecure { "http" } else { "https" };

        let timestamp = match client.get(format!("{}://{}/timestamp", http_prefix, base_url)).send().await {
            Ok(response) => {
                match response.text().await {
                    Ok(ts) => {
                        match ts.parse::<u64>() {
                            Ok(timestamp) => timestamp,
                            Err(_) => {
                                eprintln!("Server response body for /timestamp failed to parse, contact admin.");
                                tokio::time::sleep(Duration::from_secs(3)).await;
                                continue;
                            }
                        }
                    }
                    Err(_) => {
                        eprintln!("Server response body for /timestamp is empty, contact admin.");
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                }
            }
            Err(_) => {
                eprintln!("Server restarting, trying again in 3 seconds...");
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };

        println!("Server Timestamp: {}", timestamp);

        let ts_msg = timestamp.to_le_bytes();
        let sig = key.sign_message(&ts_msg);

        ws_url_str.push_str(&format!("?timestamp={}", timestamp));
        let url = url::Url::parse(&ws_url_str).expect("Failed to parse server url");
        let host = url.host_str().expect("Invalid host in server url");
        let min_difficulty = args.expected_min_difficulty;

        let auth = BASE64_STANDARD.encode(format!("{}:{}", key.pubkey(), sig));

        println!("Connecting to server...");
        let request = Request::builder()
            .method("GET")
            .uri(url.to_string())
            .header("Sec-Websocket-Key", generate_key())
            .header("Host", host)
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .header("Sec-Websocket-Version", "13")
            .header("Authorization", format!("Basic {}", auth))
            .body(())
            .unwrap();

        match connect_async(request).await {
            Ok((ws_stream, _)) => {
                println!("Connected to network!");

                let (mut sender, mut receiver) = ws_stream.split();
                let (message_sender, mut message_receiver) = unbounded_channel::<ServerMessage>();

                let receiver_thread = tokio::spawn(async move {
                    while let Some(Ok(message)) = receiver.next().await {
                        if process_message(message, message_sender.clone()).is_break() {
                            break;
                        }
                    }
                });

                // send Ready message
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();

                let msg = now.to_le_bytes();
                let sig = key.sign_message(&msg).to_string().as_bytes().to_vec();
                let mut bin_data: Vec<u8> = Vec::with_capacity(1 + 32 + 8 + sig.len());
                bin_data.push(0u8);
                bin_data.extend_from_slice(&key.pubkey().to_bytes());
                bin_data.extend_from_slice(&msg);
                bin_data.extend(sig);

                let _ = sender.send(Message::Binary(bin_data)).await;

                // receive messages
                while let Some(msg) = message_receiver.recv().await {
                    match msg {
                        ServerMessage::StartMining(challenge, nonce_range, cutoff) => {
                            println!("Received start mining message!");
                            println!("Mining starting (Using Protomine)...");
                            println!("Nonce range: {} - {}", nonce_range.start, nonce_range.end);
                            let hash_timer = Instant::now();
                            
                            let cutoff_time = cutoff;  // Use the provided cutoff directly
                            let global_best_difficulty = AtomicU32::new(0);
                            let adaptive_min_difficulty = AtomicU32::new(min_difficulty);

                            let (best_nonce, best_difficulty, best_hash, total_nonces_checked) = optimized_mining_rayon(
                                &challenge,
                                nonce_range,
                                cutoff_time,
                                &global_best_difficulty,
                                &adaptive_min_difficulty,
                                cores,
                            );

                            let hash_time = hash_timer.elapsed();

                            println!("Found best diff: {}", best_difficulty);
                            println!("Processed: {}", total_nonces_checked);
                            println!("Hash time: {:?}", hash_time);
                            println!("Hashpower: {:?} H/s", total_nonces_checked.saturating_div(hash_time.as_secs()));
                            println!("Final adaptive min difficulty: {}", adaptive_min_difficulty.load(Ordering::Relaxed));

                            let message_type = 2u8; // 2 u8 - BestSolution Message
                            let best_hash_bin = best_hash.d; // 16 u8
                            let best_nonce_bin = best_nonce.to_le_bytes(); // 8 u8
                            
                            let mut hash_nonce_message = [0; 24];
                            hash_nonce_message[0..16].copy_from_slice(&best_hash_bin);
                            hash_nonce_message[16..24].copy_from_slice(&best_nonce_bin);
                            let signature = key.sign_message(&hash_nonce_message).to_string().as_bytes().to_vec();

                            let mut bin_data = Vec::with_capacity(57 + signature.len());
                            bin_data.extend_from_slice(&message_type.to_le_bytes());
                            bin_data.extend_from_slice(&best_hash_bin);
                            bin_data.extend_from_slice(&best_nonce_bin);
                            bin_data.extend_from_slice(&key.pubkey().to_bytes());
                            bin_data.extend(signature);

                            let _ = sender.send(Message::Binary(bin_data)).await;

                            tokio::time::sleep(Duration::from_secs(3)).await;


                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .expect("Time went backwards")
                                .as_secs();

                            let msg = now.to_le_bytes();
                            let sig = key.sign_message(&msg).to_string().as_bytes().to_vec();
                            let mut bin_data = Vec::with_capacity(1 + 32 + 8 + sig.len());
                            bin_data.push(0u8);
                            bin_data.extend_from_slice(&key.pubkey().to_bytes());
                            bin_data.extend_from_slice(&msg);
                            bin_data.extend(sig);

                            let _ = sender.send(Message::Binary(bin_data)).await;
                        }
                    }
                }

                let _ = receiver_thread.await;
            }
            Err(e) => {
                match e {
                    tokio_tungstenite::tungstenite::Error::Http(e) => {
                        if let Some(body) = e.body() {
                            eprintln!("Error: {:?}", String::from_utf8_lossy(body));
                        } else {
                            eprintln!("Http Error: {:?}", e);
                        }
                    }
                    _ => {
                        eprintln!("Error: {:?}", e);
                    }
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

use std::ops::ControlFlow;

fn process_message(
    msg: Message,
    message_channel: UnboundedSender<ServerMessage>,
) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t) => {
            println!("\n>>> Server Message: \n{}\n", t);
        }
        Message::Binary(b) => {
            if b.is_empty() {
                println!("Received empty binary message");
                return ControlFlow::Continue(());
            }
            let message_type = b[0];
            match message_type {
                0 => {
                    if b.len() < 57 {
                        println!("Invalid data for Message StartMining");
                    } else {
                        let mut hash_bytes = [0u8; 32];
                        // extract 256 bytes (32 u8's) from data for hash
                        let mut b_index = 1;
                        for i in 0..32 {
                            hash_bytes[i] = b[i + b_index];
                        }
                        b_index += 32;

                        // extract 64 bytes (8 u8's)
                        let mut cutoff_bytes = [0u8; 8];
                        for i in 0..8 {
                            cutoff_bytes[i] = b[i + b_index];
                        }
                        b_index += 8;
                        let cutoff = u64::from_le_bytes(cutoff_bytes);

                        let mut nonce_start_bytes = [0u8; 8];
                        for i in 0..8 {
                            nonce_start_bytes[i] = b[i + b_index];
                        }
                        b_index += 8;
                        let nonce_start = u64::from_le_bytes(nonce_start_bytes);

                        let mut nonce_end_bytes = [0u8; 8];
                        for i in 0..8 {
                            nonce_end_bytes[i] = b[i + b_index];
                        }
                        let nonce_end = u64::from_le_bytes(nonce_end_bytes);

                        let msg = ServerMessage::StartMining(hash_bytes, nonce_start..nonce_end, cutoff);

                        let _ = message_channel.send(msg);
                    }
                }
                _ => {
                    println!("Failed to parse server message type");
                }
            }
        }
        Message::Ping(v) => {
            println!("Got Ping: {:?}", v);
        }
        Message::Pong(v) => {
            println!("Got Pong: {:?}", v);
        }
        Message::Close(v) => {
            println!("Got Close: {:?}", v);
            return ControlFlow::Break(());
        }
        _ => {
            println!("Got invalid message data");
        }
    }

    ControlFlow::Continue(())
}
