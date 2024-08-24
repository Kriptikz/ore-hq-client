use clap::{Parser, Subcommand};
use inquire::{Text, Confirm, Select};
use dirs::home_dir;
use std::path::PathBuf;
use std::io::{self, Read, Write, BufRead};
use solana_sdk::signature::read_keypair_file;
use signup::signup;
use claim::ClaimArgs;
use mine::{MineArgs, mine};
use protomine::{MineArgs as ProtoMineArgs, protomine};
use balance::balance;
use colored::*;
use std::fs;

mod signup;
mod protomine;
mod mine;
mod claim;
mod balance;

const CONFIG_FILE: &str = "keypair_list";

/// A command line interface tool for pooling power to submit hashes for proportional ORE rewards
#[derive(Parser, Debug)]
#[command(version, author, about, long_about = None)]
struct Args {
    #[arg(
        long,
        value_name = "SERVER_URL",
        help = "URL of the server to connect to",
        default_value = "ec1ipse.me",
    )]
    url: String,

    #[arg(
        long,
        value_name = "KEYPAIR_PATH",
        help = "Filepath to keypair to use",
        default_value = "~/.config/solana/id.json",
    )]
    keypair: String,

    #[arg(
        long,
        short,
        action,
        help = "Use unsecure http connection instead of https.",
    )]
    use_http: bool,

    #[command(subcommand)]
    command: Option<Commands>
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
}

#[tokio::main]
async fn main() {
    let mut args = Args::parse();

    // Does the config file exist? If not, create one
    let config_path = PathBuf::from(CONFIG_FILE);
    if !config_path.exists() {
        fs::File::create(&config_path).expect("Failed to create configuration file.");
    }

    // Check if a command was provided during runtime
    if let Some(command) = args.command.take() {
        let keypair_path = get_keypair_path(&args.keypair).expect("Failed to get keypair path.");
        let key = read_keypair_file(&keypair_path)
            .expect(&format!("Failed to load keypair from file: {}", keypair_path));

        let base_url = args.url.clone();
        let unsecure_conn = args.use_http;

        // If a command is provided, run it and exit
        if let Err(_) = run_command(Some(command), key, base_url, unsecure_conn, None).await {
            println!("An error occurred while executing the command.");
        }
        return;
    }

    // If no command is provided, enter the menu loop
    loop {
        if let Err(_) = run_menu().await {
            println!("An error occurred, returning to the main menu...");
        }
    }
}

fn get_keypair_path(default_keypair: &str) -> Option<String> {
    let config_path = PathBuf::from(CONFIG_FILE);
    let mut keypair_paths = Vec::new();

    if config_path.exists() {
        let file = fs::File::open(&config_path).expect("Failed to open configuration file.");
        let reader = io::BufReader::new(file);

        for line in reader.lines() {
            if let Ok(path) = line {
                let expanded_path = expand_tilde(&path);
                let display_path = replace_home_with_tilde(&expanded_path);
                keypair_paths.push(display_path);
            }
        }
    }

    // If the keypair list is empty, skip the selection and directly ask for a custom path
    if keypair_paths.is_empty() {
        return ask_for_custom_keypair();
    }

    keypair_paths.push("  Custom".to_string());
    keypair_paths.push("  Remove".to_string());

    // Ask the user to select a keypair
    let selection = Select::new("Select a keypair to use or manage:", keypair_paths)
        .prompt()
        .expect("Failed to prompt for keypair selection.");

    match selection.as_str() {
        "  Custom" => ask_for_custom_keypair(),
        "  Remove" => {
            remove_keypair();
            // After removal, re-run the selection process
            return get_keypair_path(default_keypair);
        }
        _ => Some(expand_tilde(&selection)),
    }
}

fn remove_keypair() {
    let config_path = PathBuf::from(CONFIG_FILE);
    let mut keypair_paths = Vec::new();

    if config_path.exists() {
        let file = fs::File::open(&config_path).expect("Failed to open configuration file.");
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
        println!("No keypairs available to remove.");
        return;
    }

    let selection = Select::new("Select a keypair to remove:", keypair_paths.clone())
        .prompt()
        .expect("Failed to prompt for keypair removal.");

    let remove_index = keypair_paths.iter().position(|p| p == &selection).unwrap();

    // Re-expand the tilde for file operations
    let removed_path = expand_tilde(&keypair_paths[remove_index]);

    keypair_paths.remove(remove_index);

    // Write the updated list back to the config file
    let mut file = fs::File::create(&config_path).expect("Failed to open configuration file for writing.");

    for path in keypair_paths {
        let expanded_path = expand_tilde(&path);
        writeln!(file, "{}", expanded_path).expect("Failed to write keypair path to configuration file.");
    }

    println!("Keypair path '{}' has been removed.", selection);
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
    let custom_path = Text::new("Enter the path to your keypair:")
        .prompt()
        .expect("Failed to get keypair path.");

    let expanded_path = expand_tilde(&custom_path);
    let custom_path_exists = PathBuf::from(&expanded_path).exists();

    if custom_path_exists {
        let add_to_list = Confirm::new("Would you like to add this keypair path to the configuration file?")
            .with_default(true)
            .prompt()
            .unwrap_or(true);

        if add_to_list {
            let config_path = PathBuf::from(CONFIG_FILE);
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&config_path)
                .expect("Failed to open configuration file for appending.");

            writeln!(file, "{}", expanded_path).expect("Failed to write keypair path to configuration file.");
        }
        Some(expanded_path)
    } else {
        println!("The specified keypair path does not exist.");
        None
    }
}

async fn run_menu() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let version = env!("CARGO_PKG_VERSION");

    let options = vec![
        "  Mine",
        "  ProtoMine",
        "  Sign up",
        "  Claim Rewards",
        "  View Balances",
        "  Stake Ore",
        "  Exit",
    ];

    println!();

    let selection = match &args.command {
        Some(_) => None, // Just execute the command if they enter it like normal
        None => Select::new(
            &format!("Welcome to Ec1ipse Ore HQ Client v{}, what would you like to do?", version), 
            options
        )
        .prompt()
        .ok(),
    };

    // Check if the user selected "Exit"
    if let Some("  Exit") = selection {
        std::process::exit(0);
    }

    if selection == Some("  Stake Ore") {
        println!("  Coming soon!");
        return Ok(());
    }

    let base_url = if args.url == "ec1ipse.me" {
        let url_input = Text::new("Please enter the server URL:")
            .with_default("ec1ipse.me")
            .prompt()
            .unwrap_or_else(|_| "ec1ipse.me".to_string());
        url_input
    } else {
        args.url.clone()
    };

    let unsecure_conn = args.use_http;

    let keypair_path = get_keypair_path(&args.keypair).expect("Failed to get keypair path.");

    // Check if the keypair file exists at the resolved path
    let keypair_exists = PathBuf::from(&keypair_path).exists();

    if keypair_exists {
        let key = read_keypair_file(&keypair_path)
            .expect(&format!("Failed to load keypair from file: {}", keypair_path));

        run_command(args.command, key, base_url, unsecure_conn, selection).await?;
        return Ok(());
    } else {
        println!("Keypair not found at the specified path.");
        return Ok(());
    }
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
        },
        Some(Commands::Protomine(args)) => {
            protomine(args, key, base_url, unsecure_conn).await;
        },
        Some(Commands::Signup) => {
            let confirm_signup = Confirm::new(&format!("{}", "Are you sure you want to sign up to the pool?".red()))
                .with_default(true)
                .prompt()
                .unwrap_or(true);

            if confirm_signup {
                signup(base_url, key, unsecure_conn).await;
                prompt_to_continue();
            } else {
                println!("Sign up cancelled.");
                prompt_to_continue();
            }
        },
        Some(Commands::Claim(args)) => {
            // Display claimable balance before prompting for the amount
            balance(&key, base_url.clone(), unsecure_conn).await;
            println!();

            let claim_amount = if let Some(amount) = args.amount {
                amount
            } else {
                loop {
                    let input = Text::new("Enter the amount to claim:").prompt().unwrap_or_default();
                    
                    match input.trim().parse::<f64>() {
                        Ok(valid_amount) => break valid_amount,
                        Err(_) => {
                            return Ok(());
                        }
                    }
                }
            };

            let confirm_claim = Confirm::new(&format!(
                "Are you sure you want to claim {} rewards?",
                claim_amount.to_string().red()
            ))
            .with_default(true)
            .prompt()
            .unwrap_or(true);

            if confirm_claim {
                let args = ClaimArgs { amount: Some(claim_amount) };
                claim::claim(args, key, base_url, unsecure_conn).await;
                prompt_to_continue();
            } else {
                println!("Claim cancelled.");
                prompt_to_continue();
            }
        },
        Some(Commands::Balance) => {
            balance(&key, base_url, unsecure_conn).await;
            prompt_to_continue();
        },
        None => {
            if let Some(choice) = selection {
                match choice {
                    "  Mine" => {
                        let threads: u32 = loop {
                            let input = Text::new("Enter the number of threads:")
                                .with_default("4")
                                .prompt()?;
            
                            match input.trim().parse::<u32>() {
                                Ok(valid_threads) if valid_threads > 0 => break valid_threads,
                                _ => {
                                    println!("Invalid input. Please enter a valid number greater than 0.");
                                }
                            }
                        };
            
                        let args = MineArgs { threads };
                        mine(args, key, base_url, unsecure_conn).await;
                    },
                    "  ProtoMine" => {
                        let threads: u32 = loop {
                            let input = Text::new("Enter the number of threads:")
                                .with_default("4")
                                .prompt()?;
            
                            match input.trim().parse::<u32>() {
                                Ok(valid_threads) if valid_threads > 0 => break valid_threads,
                                _ => {
                                    println!("Invalid input. Please enter a valid number greater than 0.");
                                }
                            }
                        };
            
                        let args = ProtoMineArgs { threads: threads.try_into().unwrap() };
                        protomine(args, key, base_url, unsecure_conn).await;
                    },            
                    "  Sign up" => {
                        let confirm_signup = Confirm::new(&format!("{}", "Are you sure you want to sign up to the pool?".red()))
                            .with_default(true)
                            .prompt()
                            .unwrap_or(true);

                        if confirm_signup {
                            signup(base_url, key, unsecure_conn).await;
                            prompt_to_continue();
                        } else {
                            println!("Sign up cancelled.");
                            prompt_to_continue();
                        }
                    },
                    "  Claim Rewards" => {
                        balance(&key, base_url.clone(), unsecure_conn).await;
                        println!();

                        let amount: f64 = loop {
                            let input = Text::new("Enter the amount to claim:").prompt().unwrap_or_default();
                            
                            match input.trim().parse::<f64>() {
                                Ok(valid_amount) => break valid_amount,
                                Err(_) => {
                                    return Ok (());
                                }
                            }
                        };

                        let confirm_claim = Confirm::new(&format!("{}", format!("Are you sure you want to claim {} rewards?", amount).red()))
                            .with_default(true)
                            .prompt()
                            .unwrap_or(true);

                        if confirm_claim {
                            let args = ClaimArgs { amount: Some(amount) };
                            claim::claim(args, key, base_url, unsecure_conn).await;
                            prompt_to_continue();
                        } else {
                            println!("Claim cancelled.");
                            prompt_to_continue();
                        }
                    },
                    "  View Balances" => {
                        balance(&key, base_url, unsecure_conn).await;
                        prompt_to_continue();
                    },
                    "  Stake Ore" => {
                        println!("Coming soon!");
                        prompt_to_continue();
                    },
                    _ => println!("Unknown selection."),
                }
            }
        },
    }

    Ok(())
}

fn prompt_to_continue() {
    println!("\nPress any key to continue...");
    let _ = io::stdin().read(&mut [0u8]).unwrap();
}
