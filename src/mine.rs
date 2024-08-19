use std::{
    ops::Range,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::prelude::*;
use clap::{arg, Parser};
use drillx::equix;
use futures_util::{SinkExt, StreamExt};
use rand::prelude::*;
use rand_chacha::ChaCha20Rng;
use rayon::prelude::*;
use solana_sdk::{signature::Keypair, signer::Signer};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        handshake::client::{generate_key, Request},
        Message,
    },
};

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
        default_value = "15"
    )]
    pub expected_min_difficulty: u32,
}

struct NonceSelectorState {
    chunk_start: u64,
    chunk_size: u64,
    current: AtomicU64,
    last_jump: AtomicU64,
}

impl NonceSelectorState {
    fn new(chunk_start: u64, chunk_size: u64) -> Self {
        Self {
            chunk_start,
            chunk_size,
            current: AtomicU64::new(chunk_start),
            last_jump: AtomicU64::new(0),
        }
    }
}

fn select_nonce(state: &NonceSelectorState, rng: &mut impl Rng) -> u64 {
    const JUMP_INTERVAL: u64 = 10000;
    
    let current = state.current.load(Ordering::Relaxed);
    let last_jump = state.last_jump.load(Ordering::Relaxed);

    if current >= state.chunk_start.wrapping_add(state.chunk_size) {
        state.current.store(state.chunk_start, Ordering::Relaxed);
        return state.chunk_start;
    }

    if current.wrapping_sub(last_jump) >= JUMP_INTERVAL {
        let new_current = state.chunk_start.wrapping_add(rng.gen_range(0..state.chunk_size));
        state.current.store(new_current, Ordering::Relaxed);
        state.last_jump.store(new_current, Ordering::Relaxed);
        return new_current;
    }

    let nonce = if rng.gen_bool(0.1) {
        state.chunk_start.wrapping_add(rng.gen_range(0..state.chunk_size))
    } else {
        current
    };

    state.current.fetch_add(1, Ordering::Relaxed);

    nonce
}

fn mine_chunk(
    challenge: &[u8; 32],
    state: &NonceSelectorState,
    cutoff_time: u64,
    global_best_difficulty: &AtomicU32,
    adaptive_min_difficulty: &AtomicU32,
    stop_signal: &AtomicBool,
    seed: u64,
) -> (u64, u32, drillx::Hash, u64) {
    let mut best_result = (0, 0, drillx::Hash::default(), 0);
    let mut total_nonces_checked = 0;
    let mut mem = equix::SolverMemory::new();
    let mut rng = ChaCha20Rng::seed_from_u64(seed);

    loop {
        if stop_signal.load(Ordering::Relaxed) {
            break;
        }

        let nonce = select_nonce(state, &mut rng);

        if SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() >= cutoff_time {
            break;
        }

        if let Ok(hx) = drillx::hash_with_memory(&mut mem, challenge, &nonce.to_le_bytes()) {
            let difficulty = hx.difficulty();
            
            if difficulty > best_result.1 {
                best_result = (nonce, difficulty, hx, total_nonces_checked + 1);
                let _ = global_best_difficulty.fetch_max(difficulty, Ordering::Release);
                let _ = adaptive_min_difficulty.fetch_max(difficulty.saturating_sub(2), Ordering::Relaxed);
            }
        }

        total_nonces_checked += 1;

        if total_nonces_checked % 1000 == 0 && stop_signal.load(Ordering::Relaxed) {
            break;
        }
    }

    best_result.3 = total_nonces_checked;
    best_result
}

fn optimized_mining(
    challenge: &[u8; 32],
    nonce_range: Range<u64>,
    cutoff_time: u64,
    global_best_difficulty: &AtomicU32,
    adaptive_min_difficulty: &AtomicU32,
    cores: usize,
) -> (u64, u32, drillx::Hash, u64) {
    let stop_signal = Arc::new(AtomicBool::new(false));
    let range_size = nonce_range.end.wrapping_sub(nonce_range.start);
    let chunk_size = range_size / cores as u64;
    
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;

    let states: Vec<_> = (0..cores)
        .map(|i| {
            let chunk_start = nonce_range.start.wrapping_add(chunk_size.wrapping_mul(i as u64));
            let chunk_end = if i == cores - 1 {
                nonce_range.end
            } else {
                chunk_start.wrapping_add(chunk_size)
            };
            NonceSelectorState::new(chunk_start, chunk_end.wrapping_sub(chunk_start))
        })
        .collect();

    let results: Vec<_> = (0..cores)
        .into_par_iter()
        .map(|i| {
            mine_chunk(
                challenge,
                &states[i],
                cutoff_time,
                global_best_difficulty,
                adaptive_min_difficulty,
                &stop_signal,
                seed.wrapping_add(i as u64),
            )
        })
        .collect();

    stop_signal.store(true, Ordering::Relaxed);

    results.into_iter().reduce(|acc, x| {
        if x.1 > acc.1 {
            (x.0, x.1, x.2, acc.3.wrapping_add(x.3))
        } else {
            (acc.0, acc.1, acc.2, acc.3.wrapping_add(x.3))
        }
    }).unwrap_or_default()
}

pub async fn mine(args: MineArgs, key: Keypair, url: String, unsecure: bool) {
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
        let cores = args.cores;
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
                            println!("Mining starting...");
                            println!("Nonce range: {} - {}", nonce_range.start, nonce_range.end);
                            let hash_timer = Instant::now();
                            
                            let cutoff_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + cutoff;
                            let global_best_difficulty = AtomicU32::new(0);
                            let adaptive_min_difficulty = AtomicU32::new(min_difficulty);

                            let (best_nonce, best_difficulty, best_hash, total_nonces_checked) = optimized_mining(
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
