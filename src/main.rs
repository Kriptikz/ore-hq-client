use std::{io::Read, ops::{ControlFlow, Range}, sync::Arc};

use drillx::equix;
use futures_util::{SinkExt, StreamExt};
use tokio::{sync::{mpsc::{UnboundedReceiver, UnboundedSender}, Mutex}, time::Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug)]
pub enum ServerMessage {
    StartMining([u8; 32], Range<u64>, u64)
}


#[tokio::main]
async fn main() {
    let url = "ws://127.0.0.1:3000/";

    println!("Connecting to url");

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");

    println!("Connected to network!");

    let (mut sender, mut receiver) = ws_stream.split();
    let (message_sender, mut message_receiver) = tokio::sync::mpsc::unbounded_channel::<ServerMessage>();

    let receiver_thread = tokio::spawn(async move {
        while let Some(Ok(message)) = receiver.next().await {
            if process_message(message, message_sender.clone()).is_break() {
                break;
            }
        }
    });

    // send Ready message
    let _ = sender.send(Message::Binary(0u8.to_le_bytes().to_vec())).await;

    let sender = Arc::new(Mutex::new(sender));

    // receive messages
    let message_sender = sender.clone();
    while let Some(msg) = message_receiver.recv().await {
        match msg {
            ServerMessage::StartMining(challenge, nonce_range, cutoff) => {
                println!("Received start mining message!");
                println!("Mining starting...");
                println!("Nonce range: {} - {}", nonce_range.start, nonce_range.end);
                let hash_timer = Instant::now();
                let threads = 32;
                let nonces_per_thread = 10_000;
                let handles = (0..threads)
                    .map(|i| {
                        std::thread::spawn({
                            let mut memory = equix::SolverMemory::new();
                            move || {
                                let first_nonce = nonce_range.start + (nonces_per_thread * i);
                                let mut nonce = first_nonce;
                                let mut best_nonce = nonce;
                                let mut best_difficulty = 0;
                                let mut best_hash = drillx::Hash::default();
                                let mut total_hashes: u64 = 0;
                                loop {
                                    total_hashes += 1;
                                    // Create hash
                                    if let Ok(hash) = drillx::hash_with_memory(
                                        &mut memory,
                                        &challenge,
                                        &nonce.to_le_bytes(),
                                    ) {
                                        let difficulty = hash.difficulty();
                                        if difficulty.gt(&best_difficulty) {
                                                best_nonce = nonce;
                                                best_difficulty = difficulty;
                                                best_hash = hash;
                                        }
                                    }

                                    // Exit if processed nonce range
                                    if nonce >= nonce_range.end {
                                        break;
                                    }

                                    if nonce % 100 == 0 {
                                        if hash_timer.elapsed().as_secs().ge(&cutoff) {
                                            break;
                                        }
                                    } 


                                    // Increment nonce
                                    nonce += 1;
                                }
                                
                                // Return the best nonce
                                Some((best_nonce, best_difficulty, best_hash, total_hashes))
                            }
                        })
                    })
                    .collect::<Vec<_>>();

                    // Join handles and return best nonce
                    let mut best_nonce: u64 = 0;
                    let mut best_difficulty = 0;
                    let mut best_hash = drillx::Hash::default();
                    let mut total_nonces_checked = 0;
                    for h in handles {
                        if let Ok(Some((nonce, difficulty, hash, nonces_checked))) = h.join() {
                            total_nonces_checked += nonces_checked;
                            if difficulty > best_difficulty {
                                best_difficulty = difficulty;
                                best_nonce = nonce;
                                best_hash = hash;
                            }
                        }
                    }

                let hash_time = hash_timer.elapsed();

                println!("Found best diff: {}", best_difficulty);
                println!("Processed: {}", total_nonces_checked);
                println!("Hash time: {:?}", hash_time);
                let message_type =  2u8; // 1 u8 - BestSolution Message
                let best_hash_bin = best_hash.d; // 16 u8
                let best_nonce_bin = best_nonce.to_le_bytes(); // 8 u8
                let mut bin_data = [0; 25];
                bin_data[00..1].copy_from_slice(&message_type.to_le_bytes());
                bin_data[01..17].copy_from_slice(&best_hash_bin);
                bin_data[17..25].copy_from_slice(&best_nonce_bin);

                {
                    let mut message_sender = message_sender.lock().await;
                    let _ = message_sender.send(Message::Binary(bin_data.to_vec())).await;
                }
            }
        }
    }


    let _ = receiver_thread.await;
    println!("Loop exited, program stopping...");
}

fn process_message(msg: Message, message_channel: UnboundedSender<ServerMessage>) -> ControlFlow<(), ()> {
    match msg {
        Message::Text(t)=>{println!("Got Text data: {:?}",t);},
        Message::Binary(b) => {
            println!("Got Binary data: {:?}", b);
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
