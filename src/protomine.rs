use std::{
    ops::{Range,ControlFlow}
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64::prelude::*;
use clap::{arg, Parser};
use futures_util::{SinkExt, StreamExt};
use rayon::prelude::*;
use solana_sdk::{signature::Keypair, signer::Signer};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        handshake::client::{generate_key, Request},
        Message,
    },
};
use std::sync::Once;
use drillx_2::equix;

static INIT_RAYON: Once = Once::new();

// Constants for tuning performance
const MIN_CHUNK_SIZE: u64 = 3_000_000;
const MAX_CHUNK_SIZE: u64 = 30_000_000;
const NONCES_PER_THREAD: u64 = 10_000;

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
            hash: drillx_2::Hash::default(),
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
        let mut memory = equix::SolverMemory::new();
        let core_range_size = (nonce_range.end - nonce_range.start) / cores as u64;
        let core_start = nonce_range.start + core_id as u64 * core_range_size;
        let core_end = if core_id == cores - 1 { nonce_range.end } else { core_start + core_range_size };
        
        let mut core_best = MiningResult::new();
        let mut local_nonces_checked = 0;

        'outer: for chunk_start in (core_start..core_end).step_by(chunk_size as usize) {
            let chunk_end = (chunk_start + chunk_size).min(core_end);
            let mut nonce = chunk_start;

            while nonce < chunk_end {
                if start_time.elapsed().as_secs() >= cutoff_time {
                    break 'outer;
                }

                if stop_signal.load(Ordering::Relaxed) {
                    break 'outer;
                }

                for _ in 0..NONCES_PER_THREAD {
                    if nonce >= chunk_end {
                        break;
                    }

                    for hx in drillx_2::get_hashes_with_memory(&mut memory, challenge, &nonce.to_le_bytes()) {
                        local_nonces_checked += 1;
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

                    nonce += 1;
                }

                if start_time.elapsed().as_secs() >= cutoff_time {
                    if core_best.difficulty >= 8 {
                        break 'outer;
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

fn process_message(msg: Message, message_channel: UnboundedSender<ServerMessage>) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t)=>{
            println!("\n>>> Server Message: \n{}\n",t);
        },
        Message::Binary(b) => {
            let message_type = b[0];
            match message_type {
                    0 => {
                        if b.len() < 49 {
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

                    },
                    _ => {
                        println!("Failed to parse server message type");
                    }
                }

        },
        Message::Ping(v) => {println!("Got Ping: {:?}", v);}, 
        Message::Pong(v) => {println!("Got Pong: {:?}", v);}, 
        Message::Close(v) => {
            println!("Got Close: {:?}", v);
            return ControlFlow::Break(());
        }, 
        _ => {println!("Got invalid message data");}
    }

    ControlFlow::Continue(())
}
