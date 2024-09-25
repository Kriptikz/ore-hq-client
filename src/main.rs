use balance::balance;
use claim::ClaimArgs;
use clap::{Parser, Subcommand};
use dirs::home_dir;
use inquire::{Confirm, Select, Text};
use mine::{mine, MineArgs};
use protomine::{protomine, MineArgs as ProtoMineArgs};
use signup::signup;
use solana_sdk::signature::read_keypair_file;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use core_affinity::get_core_ids;

mod balance;
mod claim;
mod delegate_stake;
mod mine;
mod protomine;
mod signup;
mod stake_balance;
mod undelegate_stake;
mod generate_key;

const CONFIG_FILE: &str = "keypair_list";

/// A command line interface tool for pooling power to submit hashes for proportional ORE rewards
#[derive(Parser, Debug)]
#[command(version, author, about, long_about = None)]
struct Args {
    #[arg(
        long,
        value_name = "SERVER_URL",
        help = "URL of the server to connect to",
        default_value = "ec1ipse.me"
    )]
    url: String,

    #[arg(
        long,
        value_name = "KEYPAIR_PATH",
        help = "Filepath to keypair to use",
        default_value = "~/.config/solana/id.json"
    )]
    keypair: String,

    #[arg(
        long,
        short,
        action,
        help = "Use unsecure http connection instead of https."
    )]
    use_http: bool,

    #[arg(
        long,
        short,
        action,
        help = "Use vim mode for menu navigation."
    )]
    vim: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(about = "Connect to pool and start mining. (Default)")]
    Mine(MineArgs),
    #[command(about = "Connect to pool and start mining using Prototype Software.")]
    Protomine(ProtoMineArgs),
    #[command(about = "Transfer SOL to the pool authority to sign up.")]
    Signup,
    #[command(about = "Claim rewards.")]
    Claim(ClaimArgs),
    #[command(about = "Display current ore token balance.")]
    Balance,
    #[command(about = "Delegate stake for the pool miner.")]
    Stake(delegate_stake::StakeArgs),
    #[command(about = "Undelegate stake from the pool miner.")]
    Unstake(undelegate_stake::UnstakeArgs),
    #[command(about = "Delegated stake balance.")]
    StakeBalance,
    #[command(about = "Generate a new solana keypair for mining.")]
    GenerateKeypair,
}

#[derive(Debug, Subcommand)]
enum StakeCommands {
    #[command(about = "Delegate stake for the pool miner.")]
    Stake(delegate_stake::StakeArgs),
    #[command(about = "Undelegate stake from the pool miner.")]
    Unstake(undelegate_stake::UnstakeArgs),
}

#[tokio::main]
async fn main() {
    let mut args = Args::parse();

    // Ensure the URL is set to the default if not provided
    if args.url.is_empty() {
        args.url = "ec1ipse.me".to_string();
    }

    // Does the config file exist? If not, create one
    let config_path = PathBuf::from(CONFIG_FILE);
    if !config_path.exists() {
        fs::File::create(&config_path).expect("Failed to create configuration file.");
    }

    // Check if keypair path is provided or fallback to the default
    let keypair_path = expand_tilde(&args.keypair);
    let keypair_exists = PathBuf::from(&keypair_path).exists();

    if keypair_exists {
        // Keypair path is provided and exists, proceed directly
        let key = read_keypair_file(&keypair_path).expect(&format!(
            "Failed to load keypair from file: {}",
            keypair_path
        ));

        if let Some(command) = args.command {
            // A valid command is provided, execute it directly
            if let Err(_) = run_command(Some(command), key, args.url, args.use_http, None).await {
                println!("  An error occurred while executing the command.");
            }
        } else {
            // No command provided, run the menu
            if let Err(_) = run_menu(args.vim).await {
                println!("  An error occurred, exiting program.");
            }
        }
    } else {
        // The keypair does not exist, proceed directly to the menu without showing an error
        if let Err(_) = run_menu(args.vim).await {
            println!("  An error occurred, exiting program.");
        }
    }
}

fn get_keypair_path(default_keypair: &str) -> Option<String> {
    let config_path = PathBuf::from(CONFIG_FILE);
    let mut keypair_paths = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    if config_path.exists() {
        let file = match fs::File::open(&config_path) {
            Ok(f) => f,
            Err(_) => {
                println!("  Failed to open configuration file.");
                return ask_for_custom_keypair();
            }
        };
        let reader = io::BufReader::new(file);

        let mut valid_keypair_paths = Vec::new();

        for line in reader.lines() {
            if let Ok(path) = line {
                let expanded_path = expand_tilde(&path);
                let path_buf = PathBuf::from(&expanded_path);

                if path_buf.exists() && !seen_paths.contains(&expanded_path) {
                    seen_paths.insert(expanded_path.clone());

                    if path_buf.is_dir() {
                        // Add all keypair files in the directory
                        for entry in fs::read_dir(path_buf).expect("Failed to read directory") {
                            let entry = entry.expect("Failed to get directory entry");
                            let file_path = entry.path();
                            if file_path.is_file() {
                                let file_path_str = file_path.to_string_lossy().to_string();
                                if !seen_paths.contains(&file_path_str) {
                                    valid_keypair_paths
                                        .push(replace_home_with_tilde(&file_path_str));
                                    seen_paths.insert(file_path_str);
                                }
                            }
                        }
                    } else {
                        valid_keypair_paths.push(replace_home_with_tilde(&expanded_path));
                    }
                }
            }
        }

        if !valid_keypair_paths.is_empty() {
            keypair_paths = valid_keypair_paths.clone();
            // Update config file with only valid paths
            let mut file = fs::File::create(&config_path)
                .expect("Failed to open configuration file for writing.");
            for path in valid_keypair_paths {
                writeln!(file, "{}", expand_tilde(&path))
                    .expect("Failed to write keypair path to configuration file.");
            }
        }
    }

    // Hardcode check for the default Solana keypair
    let solana_default_keypair = expand_tilde("~/.config/solana/id.json");
    if PathBuf::from(&solana_default_keypair).exists()
        && !seen_paths.contains(&solana_default_keypair)
    {
        keypair_paths.push(replace_home_with_tilde(&solana_default_keypair));
        seen_paths.insert(solana_default_keypair);
    }

    if keypair_paths.is_empty() {
        return ask_for_custom_keypair();
    }

    keypair_paths.push("  Custom".to_string());
    keypair_paths.push("  Remove".to_string());

    loop {
        let selection = match Select::new(
            "  Select a keypair to use or manage:",
            keypair_paths.clone(),
        )
        .prompt()
        {
            Ok(s) => s,
            Err(inquire::error::InquireError::OperationCanceled) => {
                println!("  Operation canceled, exiting program.");
                std::process::exit(0);
            }
            Err(_) => {
                println!("  Failed to prompt for keypair selection.");
                return None;
            }
        };

        match selection.as_str() {
            "  Custom" => return ask_for_custom_keypair(),
            "  Remove" => {
                remove_keypair();
                return get_keypair_path(default_keypair);
            }
            _ => {
                let selected_path = expand_tilde(&selection);
                if PathBuf::from(&selected_path).exists() {
                    if load_keypair(&selected_path).is_some() {
                        return Some(selected_path);
                    } else {
                        println!("  Please select a valid keypair.");
                        continue;
                    }
                } else {
                    println!(
                        "  The specified keypair path does not exist. Please enter a valid path."
                    );
                    return ask_for_custom_keypair();
                }
            }
        }
    }
}

fn remove_keypair() {
    let config_path = PathBuf::from(CONFIG_FILE);
    let mut keypair_paths = Vec::new();

    let solana_default_keypair = expand_tilde("~/.config/solana/id.json");

    if config_path.exists() {
        let file = fs::File::open(&config_path).expect("  Failed to open configuration file.");
        let reader = io::BufReader::new(file);

        for line in reader.lines() {
            if let Ok(path) = line {
                let expanded_path = expand_tilde(&path);
                let display_path = replace_home_with_tilde(&expanded_path);
                keypair_paths.push(display_path);
            }
        }
    }

    if keypair_paths.is_empty() {
        println!("  No keypairs available to remove.");
        return;
    }

    let selection =
        match Select::new("  Select a keypair to remove:", keypair_paths.clone()).prompt() {
            Ok(s) => s,
            Err(inquire::error::InquireError::OperationCanceled) => {
                println!("  Operation canceled, exiting program.");
                std::process::exit(0);
            }
            Err(_) => {
                println!("  Failed to prompt for keypair removal.");
                return;
            }
        };

    // Check if the user is trying to remove the default keypair
    if selection == replace_home_with_tilde(&solana_default_keypair) {
        println!("  Removal of the default keypair (id.json) is not allowed.");
        return;
    }

    let remove_index = keypair_paths.iter().position(|p| p == &selection).unwrap();

    keypair_paths.remove(remove_index);

    // Write the updated list back to the config file
    let mut file =
        fs::File::create(&config_path).expect("Failed to open configuration file for writing.");

    for path in keypair_paths {
        let expanded_path = expand_tilde(&path);
        writeln!(file, "{}", expanded_path)
            .expect("Failed to write keypair path to configuration file.");
    }

    println!("  Keypair path '{}' has been removed.", selection);
}

fn replace_home_with_tilde(path: &str) -> String {
    if let Some(home_dir) = home_dir() {
        let home_dir_str = home_dir.to_string_lossy();
        if path.starts_with(&*home_dir_str) {
            return path.replacen(&*home_dir_str, "~", 1);
        }
    }
    path.to_string()
}

fn expand_tilde(path: &str) -> String {
    if path.starts_with("~") {
        if let Some(home_dir) = home_dir() {
            return path.replacen("~", &home_dir.to_string_lossy(), 1);
        }
    }
    path.to_string()
}

fn ask_for_custom_keypair() -> Option<String> {
    loop {
        let custom_path = Text::new("  Enter the path to your keypair or keypair directory:")
            .prompt()
            .expect("Failed to get keypair path.");

        let expanded_path = expand_tilde(&custom_path);
        let custom_path_exists = PathBuf::from(&expanded_path).exists();

        if !custom_path_exists {
            println!("  The specified keypair path does not exist.");
            continue;
        }

        let path_buf = PathBuf::from(&expanded_path);
        if path_buf.is_dir() {
            let mut keypair_files = Vec::new();

            // Gather all .json keypair files in the directory
            for entry in fs::read_dir(&path_buf).expect("Failed to read directory") {
                let entry = entry.expect("Failed to get directory entry");
                let file_path = entry.path();
                if file_path.is_file()
                    && file_path.extension().and_then(|s| s.to_str()) == Some("json")
                {
                    let file_path_str = file_path.to_string_lossy().to_string();
                    keypair_files.push(expand_tilde(&file_path_str));
                }
            }

            if keypair_files.is_empty() {
                println!("  No .json keypair files found in the specified directory.");
                continue;
            }

            // Read and normalize existing paths from the configuration file
            let config_path = PathBuf::from(CONFIG_FILE);
            let mut existing_paths = Vec::new();
            if config_path.exists() {
                let file =
                    fs::File::open(&config_path).expect("Failed to open configuration file.");
                let reader = io::BufReader::new(file);
                for line in reader.lines() {
                    if let Ok(path) = line {
                        existing_paths.push(expand_tilde(&path));
                    }
                }
            }

            // Normalize paths for comparison
            let original_count = keypair_files.len();
            keypair_files.retain(|path| !existing_paths.contains(&expand_tilde(path)));
            let new_count = keypair_files.len();

            println!(
                "  Found {} keypair file(s) in the directory. After removing duplicates, {} new keypair file(s) remain.",
                original_count, new_count
            );

            if keypair_files.is_empty() {
                println!(
                    "  No new keypair files to add or select. Returning to the previous menu."
                );
                return None; // Returning `None` to indicate no new keypairs were selected
            }

            // Update the configuration file with unique paths
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&config_path)
                .expect("Failed to open configuration file for appending.");
            for path in &keypair_files {
                writeln!(file, "{}", expand_tilde(path))
                    .expect("Failed to write keypair path to configuration file.");
            }

            // Prompt the user to select a keypair from the directory
            let selection = match Select::new(
                "  Select a keypair to use from the directory:",
                keypair_files.clone(),
            )
            .prompt()
            {
                Ok(s) => s,
                Err(inquire::error::InquireError::OperationCanceled) => {
                    println!("  Operation canceled, exiting program.");
                    std::process::exit(0);
                }
                Err(_) => {
                    println!("  Failed to prompt for keypair selection.");
                    continue;
                }
            };

            let selected_path = expand_tilde(&selection);
            if PathBuf::from(&selected_path).exists() {
                if load_keypair(&selected_path).is_some() {
                    return Some(selected_path);
                } else {
                    println!("  Please select a valid keypair.");
                    continue;
                }
            } else {
                println!("  The specified keypair path does not exist. Please enter a valid path.");
                continue;
            }
        } else {
            if check_keypair_exists(&expanded_path) {
                println!("  The keypair path '{}' already exists in the configuration file. Please provide a new one.", custom_path);
                continue;
            }

            let add_to_list = Confirm::new(
                "  Would you like to add this keypair path to the configuration file?",
            )
            .with_default(true)
            .prompt()
            .unwrap_or(true);

            if add_to_list {
                let config_path = PathBuf::from(CONFIG_FILE);
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&config_path)
                    .expect("Failed to open configuration file for appending.");

                writeln!(file, "{}", expanded_path)
                    .expect("Failed to write keypair path to configuration file.");
            }

            return Some(expanded_path);
        }
    }
}

fn check_keypair_exists(path: &str) -> bool {
    let config_path = PathBuf::from(CONFIG_FILE);

    if config_path.exists() {
        let file = fs::File::open(&config_path).expect("Failed to open configuration file.");
        let reader = io::BufReader::new(file);

        for line in reader.lines() {
            if let Ok(existing_path) = line {
                if expand_tilde(&existing_path) == path {
                    return true;
                }
            }
        }
    }

    false
}

fn load_keypair(keypair_path: &str) -> Option<solana_sdk::signature::Keypair> {
    use std::panic::{self, AssertUnwindSafe};

    let result = panic::catch_unwind(AssertUnwindSafe(|| read_keypair_file(keypair_path)));

    match result {
        Ok(Ok(keypair)) => Some(keypair),
        Ok(Err(_)) | Err(_) => {
            println!("  Failed to load keypair from file: {}", keypair_path);
            None
        }
    }
}

async fn run_menu(vim_mode: bool) -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let version = env!("CARGO_PKG_VERSION");

    let options = vec![
        "  Mine",
        // "  ProtoMine (Experimental)",
        "  Sign up",
        "  Claim Rewards",
        "  View Balances",
        "  Stake",
        "  Unstake",
        "  Exit",
    ];

    println!();

    let selection = match &args.command {
        Some(_) => None,
        None => match Select::new(
            &format!(
                "Welcome to Ec1ipse Ore HQ Client v{}, what would you like to do?",
                version
            ),
            options,
        )
        .with_page_size(9) // Set page size to 9 for the main menu
        .with_vim_mode(vim_mode)
        .prompt()
        {
            Ok(s) => Some(s),
            Err(inquire::error::InquireError::OperationCanceled) => {
                println!("  Operation canceled, exiting program.");
                std::process::exit(0);
            }
            Err(_) => {
                println!("  Failed to prompt for selection.");
                return Err("  Failed to prompt for selection.".into());
            }
        },
    };

    if let Some("  Exit") = selection {
        std::process::exit(0);
    }

    let base_url = if args.url == "ec1ipse.me" {
        let url_input = Text::new("  Please enter the server URL:")
            .with_default("ec1ipse.me")
            .prompt()
            .unwrap_or_else(|_| "ec1ipse.me".to_string());
        url_input
    } else {
        args.url.clone()
    };

    let unsecure_conn = args.use_http;

    let keypair_path = loop {
        match get_keypair_path(&args.keypair) {
            Some(path) => break path,
            None => println!("  Failed to get keypair path. Please try again."),
        }
    };

    let key = load_keypair(&keypair_path).unwrap_or_else(|| {
        println!("  Returning to keypair selection.");
        std::process::exit(1);
    });

    run_command(
        args.command,
        key,
        base_url,
        unsecure_conn,
        selection.as_deref(),
    )
    .await?;
    Ok(())
}

async fn run_command(
    command: Option<Commands>,
    key: solana_sdk::signature::Keypair,
    base_url: String,
    unsecure_conn: bool,
    selection: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Some(Commands::Mine(args)) => {
            mine(args, key, base_url, unsecure_conn).await;
        }
        Some(Commands::Protomine(args)) => {
            protomine(args, key, base_url, unsecure_conn).await;
        }
        Some(Commands::Signup) => {
            signup(base_url, key, unsecure_conn).await;
        }
        Some(Commands::Claim(args)) => {
            claim::claim(args, key, base_url, unsecure_conn).await;
        }
        Some(Commands::Balance) => {
            balance(&key, base_url, unsecure_conn).await;
        }
        Some(Commands::Stake(args)) => {
            delegate_stake::delegate_stake(args, key, base_url, unsecure_conn).await;
        }
        Some(Commands::Unstake(args)) => {
            undelegate_stake::undelegate_stake(args, &key, base_url, unsecure_conn).await;
        }
        Some(Commands::StakeBalance) => {
            stake_balance::stake_balance(&key, base_url, unsecure_conn).await;
        }
        Some(Commands::GenerateKeypair) => {
            generate_key::generate_key().await;
        }
        None => {
            if let Some(choice) = selection {
                match choice {
                    "  Mine" => {
                        let core_ids = get_core_ids().unwrap();
                        let max_threads = core_ids.len();

                        // Ask for the number of threads
                        let threads: u32 = loop {
                            let input = Text::new(&format!(
                                "  Enter the number of threads (default: {}):", max_threads
                            ))
                            .with_default(&max_threads.to_string())
                            .prompt()?;

                            match input.trim().parse::<u32>() {
                                Ok(valid_threads) if valid_threads > 0 && valid_threads <= max_threads as u32 => break valid_threads,
                                _ => {
                                    println!("  Invalid thread count. Please enter a number between 1 and {}.", max_threads);
                                }
                            }
                        };

                        // Ask for buffer time
                        let buffer: u32 = loop {
                            let buffer_input = Text::new("  Enter the buffer time in seconds (optional):")
                                .with_default("0")
                                .prompt()?;

                            match buffer_input.trim().parse::<u32>() {
                                Ok(valid_buffer) => break valid_buffer,
                                _ => {
                                    println!("  Invalid buffer input. Please enter a valid number.");
                                }
                            }
                        };

                        let args = MineArgs { threads, buffer };
                        mine(args, key, base_url, unsecure_conn).await;
                    }

                    "  ProtoMine" => {
                        let threads: u32 = loop {
                            let input = Text::new("  Enter the number of threads:")
                                .with_default("4")
                                .prompt()?;

                            match input.trim().parse::<u32>() {
                                Ok(valid_threads) if valid_threads > 0 => break valid_threads,
                                _ => {
                                    println!("  Invalid input. Please enter a valid number greater than 0.");
                                }
                            }
                        };

                        let args = ProtoMineArgs {
                            threads: threads.try_into().unwrap(),
                        };
                        protomine(args, key, base_url, unsecure_conn).await;
                    }
                    "  Sign up" => {
                        signup(base_url, key, unsecure_conn).await;
                    }
                    "  Claim Rewards" => {
                        let args = ClaimArgs { amount: None, y: false };
                        claim::claim(args, key, base_url, unsecure_conn).await;
                    }
                    "  View Balances" => {
                        balance(&key, base_url, unsecure_conn).await;
                    }
                    "  Stake" => {
                        balance(&key, base_url.clone(), unsecure_conn).await;

                        loop {
                            let stake_input = Text::new(
                                "  Enter the amount of ore to stake (or 'esc' to cancel):",
                            )
                            .prompt();

                            match stake_input {
                                Ok(input) => {
                                    let input = input.trim();
                                    if input.eq_ignore_ascii_case("esc") {
                                        println!("  Staking operation canceled.");
                                        break;
                                    }

                                    match input.parse::<f64>() {
                                        Ok(stake_amount) if stake_amount > 0.0 => {
                                            let args = delegate_stake::StakeArgs {
                                                amount: stake_amount,
                                                auto: true, // Auto-staking by default
                                            };
                                            delegate_stake::delegate_stake(
                                                args,
                                                key,
                                                base_url.clone(),
                                                unsecure_conn,
                                            )
                                            .await;
                                            break;
                                        }
                                        Ok(_) => {
                                            println!(
                                                "  Please enter a valid number greater than 0."
                                            );
                                        }
                                        Err(_) => {
                                            println!("  Please enter a valid number.");
                                        }
                                    }
                                }
                                Err(inquire::error::InquireError::OperationCanceled) => {
                                    println!("  Staking operation canceled.");
                                    break;
                                }
                                Err(_) => {
                                    println!("  Invalid input. Please try again.");
                                }
                            }
                        }
                    }

                    "  Unstake" => {
                        stake_balance::stake_balance(&key, base_url.clone(), unsecure_conn).await;

                        loop {
                            let unstake_input = Text::new(
                                "  Enter the amount of ore to unstake (or 'esc' to cancel):",
                            )
                            .prompt();

                            match unstake_input {
                                Ok(input) => {
                                    let input = input.trim();
                                    if input.eq_ignore_ascii_case("esc") {
                                        println!("  Unstaking operation canceled.");
                                        break;
                                    }

                                    match input.parse::<f64>() {
                                        Ok(unstake_amount) if unstake_amount > 0.0 => {
                                            let args = undelegate_stake::UnstakeArgs {
                                                amount: unstake_amount,
                                            };
                                            undelegate_stake::undelegate_stake(
                                                args,
                                                &key,
                                                base_url.clone(),
                                                unsecure_conn,
                                            )
                                            .await;
                                            break;
                                        }
                                        Ok(_) => {
                                            println!(
                                                "  Please enter a valid number greater than 0."
                                            );
                                        }
                                        Err(_) => {
                                            println!("  Please enter a valid number.");
                                        }
                                    }
                                }
                                Err(inquire::error::InquireError::OperationCanceled) => {
                                    println!("  Unstaking operation canceled.");
                                    break;
                                }
                                Err(_) => {
                                    println!("  Invalid input. Please try again.");
                                }
                            }
                        }
                    }
                    _ => println!("  Unknown selection."),
                }
            }
        }
    }

    Ok(())
}
