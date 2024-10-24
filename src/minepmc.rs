use base64::prelude::*;
use drillx_2::equix;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use http::Method;
use http::header::{SEC_WEBSOCKET_KEY, HOST, SEC_WEBSOCKET_VERSION, AUTHORIZATION, UPGRADE, CONNECTION};
use indicatif::{ProgressBar, ProgressStyle};
use solana_sdk::{signature::Keypair, signer::Signer};
use spl_token::amount_to_ui_amount;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    ops::ControlFlow,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{mpsc::UnboundedSender, Mutex};
use tokio::time::timeout;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        handshake::client::{generate_key, Request},
        Message,
    },
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use colored::*;
use chrono::prelude::*;
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, AtomicU64};

use crate::database::{AppDatabase, PoolSubmissionResult};
use crate::mine::{
	MineArgs,
	ServerMessagePoolSubmissionResult, 
	ServerMessage,
	MessageSubmissionSystem,
	ThreadSubmission,
};
use crate::stats::{
	get_elapsed_string, get_miner_accuracy, record_miner_accuracy,
	set_no_more_submissions, is_transaction_in_progress, record_tx_started, record_tx_complete,
	get_global_pass_start_time, set_global_pass_start_time,
};

pub async fn minepmc(args: MineArgs, passedkey: Keypair, url: String, unsecure: bool) {
    let running = Arc::new(AtomicBool::new(true));

	let key = Arc::new(passedkey);
	
	let ms_dimmed=("ms").dimmed();
	
	let mut pass_start_time: Instant = Instant::now();
	let mining_pass = Arc::new(AtomicU64::new(0)); // Create an atomic counter	
	
	// OVERMINE_BY_MS: The pool server allow several secs by default between finishing mining & signing your submission. 
	// overmine_by_ms allows shortening this duration to enable up until the server has started to submit the transaction.
	let overmine_by_ms_str=env::var("OVERMINE_BY_MS").unwrap_or("4200".to_string());
	let overmine_by_ms: u64 = overmine_by_ms_str.parse().unwrap_or(4200);
	println!("        Setting overmine_by_ms duration to {}{}", overmine_by_ms.to_string().blue(), ms_dimmed);

	// NONCE_INIT_INTERVAL: This value is used in the calculation to guestimate how long your miner takes to do a hash.
	// It is used to tune how accurate you can end your mining time to a precise time
	// A higher interval is better (~1% of your processed count)
	// Aim for an accuracy of <50ms on average
	let nonce_init_interval_str=env::var("NONCE_INIT_INTERVAL").unwrap_or("100".to_string());
	let nonce_init_interval: u64 = nonce_init_interval_str.parse().unwrap_or(100);
	println!("        Setting nonce_init_interval to {}", nonce_init_interval.to_string().blue());

	// CORE_OFFSET: An offset so that you can begin the mining threads starting from the CORE_OFFSET value. 
	// This allows you to potentially run multiple miners on the same machine but not tie them to all start threads on core 0
	let core_offset_str=env::var("CORE_OFFSET").unwrap_or("0".to_string());
	let core_offset: u32 = core_offset_str.parse().unwrap_or(0);
	println!("        Setting core_offset to {}", core_offset.to_string().blue());

    loop {
		let connection_started=Instant::now();

		if !running.load(Ordering::SeqCst) {
            break;
        }

        let base_url = url.clone();
        let mut ws_url_str = if unsecure {
            format!("ws://{}/v2/ws", url)
        } else {
            format!("wss://{}/v2/ws", url)
        };

        // let client = reqwest::Client::new();
		let client = reqwest::Client::builder()
				.timeout(Duration::from_secs(2))
				.tcp_nodelay(true)  // Disable Nagle's algorithm
				.tcp_keepalive(Some(Duration::from_secs(60)))
				.pool_idle_timeout(Some(Duration::from_secs(30)))
				.pool_max_idle_per_host(5)
				.build()
				.expect("Failed to setup client connection");

        let http_prefix = if unsecure {
            "http".to_string()
        } else {
            "https".to_string()
        };

        let timestamp = match client
            .get(format!("{}://{}/timestamp", http_prefix, base_url))
            .send()
            .await
        {
            Ok(res) => {
                if res.status().as_u16() >= 200 && res.status().as_u16() < 300 {
                    if let Ok(ts) = res.text().await {
                        if let Ok(ts) = ts.parse::<u64>() {
                            ts
                        } else {
                            println!("Server response body for /timestamp failed to parse, contact admin.");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                            continue;
                        }
                    } else {
                        println!("Server response body for /timestamp is empty, contact admin.");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                } else {
                    println!(
                        "Failed to get timestamp from server. StatusCode: {}",
                        res.status()
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            }
            Err(e) => {
                println!("Failed to get timestamp from server.\nError: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        println!("\tServer Timestamp: {}", timestamp);

        let ts_msg = timestamp.to_le_bytes();
        let sig = key.sign_message(&ts_msg);

        ws_url_str.push_str(&format!("?timestamp={}", timestamp));
        let url = url::Url::parse(&ws_url_str).expect("Failed to parse server url");
        let host = url.host_str().expect("Invalid host in server url");
        let threads = args.threads;

        let auth = BASE64_STANDARD.encode(format!("{}:{}", key.pubkey(), sig));

        println!("\tConnecting to server...");
        let websocket_request = Request::builder()
			.method(Method::GET)
			.uri(url.to_string())
			.header(UPGRADE, "websocket")
			.header(CONNECTION, "Upgrade")
			.header(SEC_WEBSOCKET_KEY, generate_key())
			.header(HOST, host)
			.header(SEC_WEBSOCKET_VERSION, "13")
			.header(AUTHORIZATION, format!("Basic {}", auth))
			.body(())
			.unwrap();

        match connect_async_with_config(websocket_request, None, true).await {
            Ok((ws_stream, _)) => {
				let elapsed_str2=get_elapsed_string(pass_start_time);
				println!("{}{}{}{}", 
					elapsed_str2, 
					"Server: ".dimmed(), 
					format!("Connected to network!").blue(),
					format!(" [{}ms]", connection_started.elapsed().as_millis()).dimmed(),
				);	

                let (sender, mut receiver) = ws_stream.split();
                let (message_sender, mut message_receiver) =
                    tokio::sync::mpsc::unbounded_channel::<ServerMessage>();

                let (solution_system_message_sender, solution_system_message_receiver) =
                    tokio::sync::mpsc::unbounded_channel::<MessageSubmissionSystem>();

                let sender = Arc::new(Mutex::new(sender));
                let app_key = key.clone();
                let app_socket_sender = sender.clone();
                tokio::spawn(async move {
                    submission_system(app_key, solution_system_message_receiver, app_socket_sender)
                        .await;
                });

                let solution_system_submission_sender = Arc::new(solution_system_message_sender);

                let msend = message_sender.clone();
                let system_submission_sender = solution_system_submission_sender.clone();
                let receiver_thread = tokio::spawn(async move {
                    let mut last_start_mine_instant = Instant::now();
                    loop {
                        match timeout(Duration::from_secs(45), receiver.next()).await {
                            Ok(Some(Ok(message))) => {
                                match process_message(message, msend.clone()) {
                                    ControlFlow::Break(_) => {
                                        break;
                                    }
                                    ControlFlow::Continue(got_start_mining) => {
                                        if got_start_mining {
                                            last_start_mine_instant = Instant::now();
                                        }
                                    }
                                }

                                if last_start_mine_instant.elapsed().as_secs() >= 120 {
                                    eprintln!("Last start mining message was over 2 minutes ago. Closing websocket for reconnection.");
                                    break;
                                }
                            }
                            Ok(Some(Err(e))) => {
                                eprintln!("Websocket error: {}", e);
                                break;
                            }
                            Ok(None) => {
                                eprintln!("Websocket closed gracefully");
                                break;
                            }
                            Err(_) => {
                                eprintln!("Websocket receiver timeout, assuming disconnection");
                                break;
                            }
                        }
                    }

                    println!("Websocket receiver closed or timed out.");
                    println!("Cleaning up channels...");
                    let _ = system_submission_sender.send(MessageSubmissionSystem::Finish);
                    drop(msend);
                    drop(message_sender);
                });

                // send Ready message
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    .as_secs();

                let msg = now.to_le_bytes();
                let sig = key.sign_message(&msg).to_string().as_bytes().to_vec();
                let mut bin_data: Vec<u8> = Vec::new();
                bin_data.push(0u8);
                bin_data.extend_from_slice(&key.pubkey().to_bytes());
                bin_data.extend_from_slice(&msg);
                bin_data.extend(sig);

                let mut lock = sender.lock().await;
                let _ = lock.send(Message::Binary(bin_data)).await;
                drop(lock);

                let (db_sender, mut db_receiver) =
                    tokio::sync::mpsc::unbounded_channel::<PoolSubmissionResult>();

                tokio::spawn(async move {
                    let app_db = AppDatabase::new();
                    while let Some(msg) = db_receiver.recv().await {
                        app_db.add_new_pool_submission(msg);
                        let total_earnings = amount_to_ui_amount(
                            app_db.get_todays_earnings(),
                            ore_api::consts::TOKEN_DECIMALS,
                        );
                        println!("\t{}", format!("Todays Earnings: {} ORE @ {} on {}", total_earnings, Local::now().format("%H:%M:%S"), Local::now().format("%Y-%m-%d")).green());
                    }
                });

				pass_start_time = Instant::now();
				set_global_pass_start_time(pass_start_time, mining_pass.load(Ordering::Relaxed));
				let mining_pass_clone = mining_pass.clone(); // Clone the Arc for the async block

                // receive messages
                let s_system_submission_sender = solution_system_submission_sender.clone();
                while let Some(msg) = message_receiver.recv().await {
                    let system_submission_sender = s_system_submission_sender.clone();
                    let db_sender = db_sender.clone();
					let mining_pass = mining_pass_clone.clone(); // Clone for the spawn
                    tokio::spawn({
                        let message_sender = sender.clone();
                        let key = key.clone();
                        let running = running.clone();
                        async move {
                            if !running.load(Ordering::SeqCst) {
                                return;
                            }
							
							// Show a name for this miner at the start of each pass - e.g. MINER_NAME=$(hostname)
							let miner_name = env::var("MINER_NAME").unwrap_or("".to_string());

							let mut elapsed_str: String;
                            match msg {
                                ServerMessage::StartMining(challenge, nonce_range, cutoff) => {
									let elapsed_str3 = get_elapsed_string(get_global_pass_start_time());
									println!("{}{} {}", 
										elapsed_str3,
										"server:".dimmed(),
										"Start mining next pass".blue(),
									); 

									let pass_start_time = Instant::now();
									let solve_start_time_local_ms = Local::now().timestamp_micros();
									let ms_dimmed=("ms").dimmed();

									let current_pass = mining_pass.fetch_add(1, Ordering::SeqCst) + 1;
									record_tx_complete();
									set_no_more_submissions(false);
									set_global_pass_start_time(pass_start_time, current_pass as u64);

									println!("\n\n{} mining pass {} [{} threads]:", miner_name.clone(), current_pass, args.threads);
									println!("{}", format!(
                                        "Next Challenge: {}",
                                        BASE64_STANDARD.encode(challenge)
                                    ).dimmed());
									println!("{}", format!(
                                        "Nonce range: {} - {}",
                                        nonce_range.start, nonce_range.end
                                    ).dimmed());
                                    
                                    // Adjust the cutoff time to accomodate a buffer 
                                    let mut cutoff = cutoff.saturating_sub(args.buffer as u64);
                                    if cutoff > 60 {
                                        cutoff = 55;
                                    }

                                    // Detect if running on Windows and set symbols accordingly
                                    let pb = if env::consts::OS == "windows" {
                                        ProgressBar::new_spinner().with_style(
                                            ProgressStyle::default_spinner()
                                                .tick_strings(&["-", "\\", "|", "/"]) // Use simple ASCII symbols
                                                .template("{spinner:.green} {msg}")
                                                .expect("Failed to set progress bar template"),
                                        )
                                    } else {
                                        ProgressBar::new_spinner().with_style(
                                            ProgressStyle::default_spinner()
                                                .tick_strings(&[
                                                    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇",
                                                    "⠏",
                                                ])
                                                .template("{spinner:.red} {msg}")
                                                .expect("Failed to set progress bar template"),
                                        )
                                    };

									// Determine how close to the cuttoff time to mine up to
									let mut cutoff = cutoff.saturating_sub(args.buffer as u64);
									if cutoff > 60 {
										cutoff = 55;
									}
									let cutoff_with_overmine=(cutoff*1_000_000)+(overmine_by_ms as u64*1000);
									let cutoff_timestamp_ms: i128 = Local::now().timestamp_micros() as i128 
													+ (cutoff_with_overmine as i128)
													- (get_miner_accuracy() * 1000.0) as i128;

									elapsed_str = get_elapsed_string(pass_start_time);
									println!("{}Mine for {:.2}s - Default: {}s", elapsed_str,
													(cutoff_with_overmine as f64 - (get_miner_accuracy()*1000.0)) / 1_000_000.0,
													cutoff,
									);
									println!("{}{}", elapsed_str,	format!("Nonce range: {} - {}", nonce_range.start, nonce_range.end).dimmed());
									let nonces_per_thread = (nonce_range.end-nonce_range.start).saturating_div(2).saturating_div(threads as u64); //10_000;
		
                                    pb.set_message("      Mining...");
                                    pb.enable_steady_tick(Duration::from_millis(120));

									let core_ids = core_affinity::get_core_ids().unwrap();

									// Best solution will be updated by each thread as better difficulties are found
									let best_solution: Arc<MiningSolution> = MiningSolution::new(Keypair::from_bytes(&key.to_bytes()).unwrap());

									// Startup Threads+1 actual threads. Extra one is a control thread
									let handles: Vec<_> = (0..threads).map(|thread_number| {									
										// Get a handle to the best_solution 
										let best_solution = Arc::clone(&best_solution);
										let system_submission_sender = system_submission_sender.clone();
										let core_id = core_ids[(thread_number + core_offset) as usize];
										let keypair_being_mined = 0;
										let builder = std::thread::Builder::new()
											.name(format!("ore_hq_cl_{}", thread_number + core_offset))
											.stack_size(256*1024);	// Attempt to reduce memory requirements for each thread
										builder.spawn({	
											move || {
												// Mining Thread
												let mut memory = equix::SolverMemory::new();
												let _ = core_affinity::set_for_current(core_id);
												// println!("Assigning thread {} to core_id {}", thread_number, core_id.id);
												let first_nonce = nonce_range.start + (nonces_per_thread * (thread_number as u64));
												let mut nonce = first_nonce;
												let mut nonces_current_interval = nonce_init_interval*2;
												let mut cutoff_nonce = nonce + nonces_current_interval;
												let mut current_nonces_per_ms: f64 ; // = 200.0;
												let mut thread_hashes: u32 = 0;
												let loop_start_time_local_ms = Local::now().timestamp_micros();
												let mut current_timestamp_ms: i64; // = Local::now().timestamp_micros();
												
												let left = cutoff_timestamp_ms-loop_start_time_local_ms as i128 / 1000000;
												let mut more_than_5_secs_left=1;
												if left<5 { more_than_5_secs_left=0; }
												
												let mut this_threads_difficulty=6;
												let mut difficulty: u32;
												let mut seed = [0_u8; 40];
												let mut equix_builder=equix::EquiXBuilder::new();
												let equix_rt = equix_builder.runtime(equix::RuntimeOption::TryCompile);
												let mut nonce_le_bytes: [u8; 8];
												seed[00..32].copy_from_slice(&challenge);
												loop {
													nonce_le_bytes=nonce.to_le_bytes();
													// let start_time=Instant::now();
													seed[32..40].copy_from_slice(&nonce_le_bytes);
													match equix_rt.build(&seed).map_err(|_| drillx_2::DrillxError::BadEquix) {
														Ok(equix) => {
															let solutions = equix.solve_with_memory(&mut memory);
															for solution in solutions {
																let digest = solution.to_bytes();
																let hash = drillx_2::hashv(&digest, &nonce_le_bytes);
																thread_hashes = thread_hashes.wrapping_add(1);
																
																// Determine the number of leading zeroes
																difficulty = 0;
																for byte in hash {
																	if byte == 0 {
																		difficulty = difficulty.wrapping_add(8);
																	} else {
																		difficulty = difficulty.wrapping_add(byte.leading_zeros());
																		break;
																	}
																}

																if difficulty>this_threads_difficulty {
																	this_threads_difficulty=difficulty;
																	let better_diff = best_solution.check_for_improved_difficulty(difficulty, nonce, digest, pass_start_time, first_nonce, keypair_being_mined);
																	if better_diff {
																		// A higher difficulty has been found since the last difficulty was sent to server 
																		// Send higher difficulty & hope it gets there before the server processes your account
																		let (_best_difficulty, _best_nonce, _best_digest, _key, _key_pubkey, _difficulty_submitted)= best_solution.read();
																		if !is_transaction_in_progress() {
																			let thread_submission = ThreadSubmission{
																				nonce,
																				difficulty: this_threads_difficulty,
																				d: digest,
																			};
																			let _ = system_submission_sender.send(MessageSubmissionSystem::Submission(thread_submission));
																		
																			best_solution.update_difficulty_submitted(this_threads_difficulty);

																		} else {
																			let elapsed_str = get_elapsed_string(pass_start_time);
																			println!("{}{}", elapsed_str, format!("Too late to submit {} ...", this_threads_difficulty).yellow());
																		}
																	}
																}
															}
														},
														Err(_err) => {
															// Handle the error case from equix
															// println!("Error with equix: {:?}", err);
														}
													}

													// Increment nonce & process only when we reach the cutoff_nonce
													nonce=nonce.wrapping_add(1);
													if nonce >= cutoff_nonce {
														current_timestamp_ms = Local::now().timestamp_micros();
														
														// Determine current nonces per ms for the duration so far
														current_nonces_per_ms = (nonce-first_nonce) as f64 / (current_timestamp_ms as i128 - loop_start_time_local_ms as i128) as f64;

														if more_than_5_secs_left>0 {		// called before the end of the mining pass - to target 5s before cutoff timestamp to ensure accurate finishing time
															nonces_current_interval = ((cutoff_timestamp_ms - current_timestamp_ms as i128 - 5_000_000) as f64 * current_nonces_per_ms) as u64;
														
														} else {							// called at 5s before the end of the mining pass - to rarget 2.5ms before cutoff timestamp
															nonces_current_interval = ((cutoff_timestamp_ms - current_timestamp_ms as i128  - 2_500) as f64 * current_nonces_per_ms) as u64;
														}
														more_than_5_secs_left-=1;

														// Set the number of the cutoff nonce where the next check for completion will take place
														cutoff_nonce = nonce.wrapping_add(nonces_current_interval);
														
														// Exit loop if <1 non to get to cutoff
														if nonces_current_interval<1 {
															// let elapsed_str = get_elapsed_string(pass_start_time);
															// println!("{}[{}] Stopping as nonces_current_interval<1: {} current_nonces_per_ms: {} ms_to_go: {}", 
															// 	elapsed_str, thread_number, nonces_current_interval, current_nonces_per_ms, (cutoff_timestamp_ms - current_timestamp_ms as i128));
															break;
														}
														// Exit if processed nonce range
														if nonce >= nonce_range.end {
															// let elapsed_str = get_elapsed_string(pass_start_time);
															// println!("{}[{}] Stopping at end of nonce range: {}", elapsed_str, thread_number, nonce_range.end);
															break;
														}

														// Exit if mining pass has ended
														if is_transaction_in_progress() {
															// let elapsed_str = get_elapsed_string(pass_start_time);
															// println!("{}[{}] Stopping as transaction is in progress", elapsed_str, thread_number);
															break;
														}
													}
												}
												
												// Return the number of hashes processed - best_solution contains best difficulty from all threads
												Some(thread_hashes)
											}
										})
									}).collect::<Vec<_>>();

									// Join handles and return best nonce
									let mut total_nonces_checked = 0;
									for h in handles {
										if let Ok(Some(/*nonce, difficulty, hash, */nonces_checked)) = h.unwrap().join() {
											total_nonces_checked += nonces_checked;
										}
									}
									let (best_difficulty, _best_nonce, _best_digest, _key, _key_pubkey, _difficulty_submitted)= best_solution.read();
									let finished_mining_local_ms=Local::now().timestamp_micros();
									let mining_took_ms = finished_mining_local_ms - solve_start_time_local_ms;
							
									// log the hash accuracy time
									let overmined_by_ms=(finished_mining_local_ms-cutoff_timestamp_ms as i64) as f64/1000.0;
									elapsed_str = get_elapsed_string(pass_start_time);
									println!("{}{}", 
										elapsed_str.clone(),
										format!("Finished mining after {:.2}s. Accuracy: {:.0}{}",
											mining_took_ms as f64 /1000000.0,
											overmined_by_ms, ms_dimmed,
										).yellow().dimmed(),
									);
									
									// Detect if end of mining pass
									if (cutoff_timestamp_ms as i64)< (Local::now().timestamp_micros()-1_000_000) {
										record_miner_accuracy(overmined_by_ms);
									}

                                    // Stop the spinner after mining is done
                                    pb.finish_and_clear();
                                    // println!("✔ Mining complete!");
                                    println!("\tProcessed: {}", total_nonces_checked);
                                    println!("\tHash time: {:.2}", mining_took_ms as f64 /1000000.0);
                                    let hash_time_secs = (mining_took_ms as f64 /1000000.0) as u32;
                                    if hash_time_secs > 0 {
                                        println!(
                                            "\tHashpower: {:?} H/s",
                                            total_nonces_checked.saturating_div(hash_time_secs)
                                        );
                                        println!("\tClient found diff: {}", best_difficulty);
                                    }

                                    let _ = system_submission_sender
                                        .send(MessageSubmissionSystem::Reset);

                                    //tokio::time::sleep(Duration::from_secs(5 + args.buffer as u64)).await;

                                    // Ready up again
                                    let now = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .expect("Time went backwards")
                                        .as_secs();

                                    let msg = now.to_le_bytes();
                                    let sig =
                                        key.sign_message(&msg).to_string().as_bytes().to_vec();
                                    let mut bin_data: Vec<u8> = Vec::new();
                                    bin_data.push(0u8);
                                    bin_data.extend_from_slice(&key.pubkey().to_bytes());
                                    bin_data.extend_from_slice(&msg);
                                    bin_data.extend(sig);
                                    {
                                        let mut message_sender = message_sender.lock().await;
                                        if let Err(_) =
                                            message_sender.send(Message::Binary(bin_data)).await
                                        {
                                            let _ = system_submission_sender
                                                .send(MessageSubmissionSystem::Finish);
                                            println!("Failed to send Ready message. Returning...");
                                            return;
                                        }
                                    }
                                }
                                ServerMessage::PoolSubmissionResult(data) => {
                                    let pool_earned = (data.total_rewards
                                        * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64))
                                        as u64;
                                    let miner_earned = (data.miner_earned_rewards
                                        * 10f64.powf(ore_api::consts::TOKEN_DECIMALS as f64))
                                        as u64;
                                    let ps = PoolSubmissionResult::new(
                                        data.difficulty,
                                        pool_earned,
                                        data.miner_percentage,
                                        data.miner_supplied_difficulty,
                                        miner_earned,
                                    );
                                    let _ = db_sender.send(ps);

                                    let message = format!(		
                                        "\n_________________________________________________________________\nPrevious Challenge: {}\nPool Submitted Difficulty: {}\t\tMiner: {}\nPool Earned:  {} ORE\tMiner: {} ORE\nPool Balance: {:.11} ORE\t{} of total pool reward\nTop Stake:    {:.11} ORE\nPool Multiplier: {:.2}x\nActive Miners:   {}\n‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾",
                                        BASE64_STANDARD.encode(data.challenge),
                                        format!("{}", data.difficulty).blue(),
                                        format!("{}", data.miner_supplied_difficulty).green(),
                                        format!("{:11}", data.total_rewards).blue(),
                                        format!("{:11}", data.miner_earned_rewards).green(),
                                        data.total_balance,
                                        format!("{:.3}%", data.miner_percentage).green(),
                                        data.top_stake,
                                        data.multiplier,
                                        data.active_miners,
                                    );
                                    println!("{}", message);
                                }
                            }
                        }
                    });
                }

                // If the websocket message receiver finishes, also finish the solution submission
                // sender system
                let _ = receiver_thread.await;
                let _ = solution_system_submission_sender.send(MessageSubmissionSystem::Finish);
                println!("Channels cleaned up, reconnecting...\n");
            }
            Err(e) => {
                match e {
                    tokio_tungstenite::tungstenite::Error::Http(e) => {
                        if let Some(body) = e.body() {
                            println!("Error: {:?}", String::from_utf8(body.to_vec()));
                        } else {
                            println!("Http Error: {:?}", e);
                        }
                    }
                    _ => {
                        println!("Error: {:?}", e);
                    }
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

fn process_message(
    msg: Message,
    message_channel: UnboundedSender<ServerMessage>,
) -> ControlFlow<(), bool> {
	let pass_start_time = get_global_pass_start_time();
	let elapsed_str = get_elapsed_string(pass_start_time);
	let mut got_start_mining_message = false;
    match msg {
        Message::Text(t) => {
			if t.starts_with("Pool Submitted") {
				println!("{}{}", elapsed_str, "Server: Rewards Received".bright_magenta());
			} else {
				println!("{}{}{}", elapsed_str, "Server: ".dimmed(), t.blue());	
			}
			if t=="Server is sending mine transaction..." {
				if !is_transaction_in_progress() {
					record_tx_started();
				}
				set_no_more_submissions(true);
			}
        }
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

                        let msg =
                            ServerMessage::StartMining(hash_bytes, nonce_start..nonce_end, cutoff);

                        let _ = message_channel.send(msg);
                        got_start_mining_message = true;
                    }
                }
                1 => {
                    let msg = ServerMessage::PoolSubmissionResult(
                        ServerMessagePoolSubmissionResult::new_from_bytes(b),
                    );
                    let _ = message_channel.send(msg);
                }
                _ => {
                    println!("Failed to parse server message type");
                }
            }
        }
        Message::Ping(_) => {}
        Message::Pong(_) => {}
        Message::Close(v) => {
            println!("Got Close: {:?}", v);
            return ControlFlow::Break(());
        }
        _ => {}
    }

    ControlFlow::Continue(got_start_mining_message)
}

async fn submission_system(
    key: Arc<Keypair>,
    mut system_message_receiver: UnboundedReceiver<MessageSubmissionSystem>,
    socket_sender: Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>,
) {
    let mut best_diff = 0;
    while let Some(msg) = system_message_receiver.recv().await {
        match msg {
            MessageSubmissionSystem::Submission(thread_submission) => {
                if thread_submission.difficulty > best_diff {
                    best_diff = thread_submission.difficulty;

                    // Send results to the server
                    let message_type = 2u8; // 1 u8 - BestSolution Message
                    let best_hash_bin = thread_submission.d; // 16 u8
                    let best_nonce_bin = thread_submission.nonce.to_le_bytes(); // 8 u8

                    let mut hash_nonce_message = [0; 24];
                    hash_nonce_message[0..16].copy_from_slice(&best_hash_bin);
                    hash_nonce_message[16..24].copy_from_slice(&best_nonce_bin);
                    let signature = key
                        .sign_message(&hash_nonce_message)
                        .to_string()
                        .as_bytes()
                        .to_vec();

                    let mut bin_data = [0; 57];
                    bin_data[00..1].copy_from_slice(&message_type.to_le_bytes());
                    bin_data[01..17].copy_from_slice(&best_hash_bin);
                    bin_data[17..25].copy_from_slice(&best_nonce_bin);
                    bin_data[25..57].copy_from_slice(&key.pubkey().to_bytes());

                    let mut bin_vec = bin_data.to_vec();
                    bin_vec.extend(signature);

                    let mut message_sender = socket_sender.lock().await;
                    let _ = message_sender.send(Message::Binary(bin_vec)).await;
                    drop(message_sender);
                }
            }
            MessageSubmissionSystem::Reset => {
                best_diff = 0;

                // Sleep for 2 seconds to let the submission window open again
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            MessageSubmissionSystem::Finish => {
                return;
            }
        }
    }
}

// SAFETY: We ensure that access to `digest` is properly synchronized
// through the `check_for_improved_difficulty` method.
unsafe impl Sync for MiningSolution {}
struct MiningSolution {
    difficulty: AtomicU32,
    difficulty_submitted: AtomicU32,
    nonce: AtomicU64,
	digest: UnsafeCell<[u8; 16]>,
	key: Keypair,
}

impl MiningSolution {
    fn new(key: Keypair) -> Arc<Self> {
		let hx=drillx_2::Hash::default();
        Arc::new(Self {
            difficulty: AtomicU32::new(0),
			difficulty_submitted: AtomicU32::new(0),
            nonce: AtomicU64::new(0),
			digest: UnsafeCell::new(hx.d),
			key,
        })
    }

    fn _update_difficulty(&self, new_difficulty: u32) {
        self.difficulty.store(new_difficulty, Ordering::Relaxed);
    }

    fn _update_nonce(&self, new_nonce: u64) {
        self.nonce.store(new_nonce, Ordering::Relaxed);
    }

	fn update_difficulty_submitted(&self, the_difficulty: u32) {
		self.difficulty_submitted.store(the_difficulty, Ordering::Relaxed);
	}

    fn read(&self) -> (u32, u64, [u8; 16], &Keypair, [u8; 32], u32) {
        let difficulty = self.difficulty.load(Ordering::Relaxed);
        let difficulty_submitted = self.difficulty_submitted.load(Ordering::Relaxed);
        let nonce = self.nonce.load(Ordering::Relaxed);
        // SAFETY: We're only reading the digest, which is safe as long as we're not writing to it
        let digest = unsafe { *self.digest.get() };
        // let key = unsafe { *self.key.get() };
        (difficulty, nonce, digest, &self.key, self.key.pubkey().to_bytes(), difficulty_submitted)
    }

	fn check_for_improved_difficulty(&self, current_difficulty: u32, current_nonce: u64, digest: [u8; 16], _pass_start_time: Instant, _first_nonce: u64, _keypair_being_mined: u32) -> bool {
        if current_difficulty > self.difficulty.load(Ordering::Relaxed) {
			if is_transaction_in_progress() {
				return false;
			}

            self.difficulty.store(current_difficulty, Ordering::Relaxed);
            self.nonce.store(current_nonce, Ordering::Relaxed);
            // SAFETY: We're ensuring single-threaded access to `digest` by checking difficulty first
			unsafe { *self.digest.get() = digest };

			println!("{}[{}{}] {} {}",
				"\x1B[1A ",
				format!("{:>4.1}", (_pass_start_time.elapsed().as_millis() as f64 / 1000.0)).dimmed(), 
				("s".dimmed()).to_string(),
				format!("Mined").dimmed(),
				format!("diff {}", current_difficulty).bright_cyan(),
				// format!("nonce {}", current_nonce-_first_nonce).cyan(),
			);
            true
        } else {
            false
        }
    }
}
