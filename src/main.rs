use clap::{Parser, Subcommand};
use inquire::{Text, Confirm, Select};
use dirs::home_dir;
use std::path::PathBuf;
use std::io::{self, Read};
use solana_sdk::signature::read_keypair_file;
use signup::signup;
use claim::ClaimArgs;
use mine::{MineArgs, mine};
use protomine::{MineArgs as ProtoMineArgs, protomine};
use balance::balance;
use colored::*;

mod signup;
mod protomine;
mod mine;
mod claim;
mod balance;

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

    // Check if a command was provided
    if let Some(command) = args.command.take() {
        // Resolve the keypair path, expanding '~' to home directory
        let keypair_path = if args.keypair.starts_with("~") {
            if let Some(home_dir) = home_dir() {
                let mut expanded_path = PathBuf::from(home_dir);
                expanded_path.push(&args.keypair[2..]); // Skip '~/' in keypair path
                expanded_path.to_string_lossy().into_owned()
            } else {
                args.keypair.clone()
            }
        } else {
            args.keypair.clone()
        };

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


async fn run_menu() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Args::parse();

    let version = env!("CARGO_PKG_VERSION");

    let options = vec![
        "Mine",
        "ProtoMine",
        "Sign up",
        "Claim Rewards",
        "View Balances",
        "Help",
        "Exit",
    ];

    println!();

    let selection = match &args.command {
        Some(_) => None, // Just execute the command if they enter it like normal
        None => Select::new(
            &format!("Welcome to Ec1ipse Ore HQ Client v{}, what would you like to do?\n", version), 
            options
        )
        .prompt()
        .ok(),
    };

    if selection == Some("Exit") {
        std::process::exit(0);
    }

    if selection == Some("Help") {
        println!("This is a command line tool to interact with the Ec1ipse Ore Mining Pool. Use the options provided to mine, check balances, claim rewards, or sign up.");
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

    // Resolve the keypair path, expanding '~' to home directory
    let keypair_path = if args.keypair.starts_with("~") {
        if let Some(home_dir) = home_dir() {
            let mut expanded_path = PathBuf::from(home_dir);
            expanded_path.push(&args.keypair[2..]); // Skip '~/' in keypair path
            expanded_path.to_string_lossy().into_owned()
        } else {
            args.keypair.clone()
        }
    } else {
        args.keypair.clone()
    };

    // Prompt the user to confirm using the default keypair path
    if keypair_path.ends_with("id.json") {
        let use_default = Confirm::new(&format!("Do you want to use the default keypair located at {}?", keypair_path))
            .with_default(true) // This sets "Yes" as the default option
            .prompt()
            .unwrap_or(true);

        if !use_default {
            let custom_keypair = Text::new("Please enter the path to your keypair:")
                .prompt()
                .unwrap();
            let key = read_keypair_file(&custom_keypair)
                .expect(&format!("Failed to load keypair from file: {}", custom_keypair));
            run_command(args.command, key, base_url, unsecure_conn, selection).await?;
            return Ok(());
        }
    }

    let key = read_keypair_file(&keypair_path)
        .expect(&format!("Failed to load keypair from file: {}", keypair_path));

    run_command(args.command, key, base_url, unsecure_conn, selection).await?;

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
            // Display claimable balance before confirming claim
            balance(&key, base_url.clone(), unsecure_conn).await;

            let confirm_claim = Confirm::new(&format!("{}", format!("Are you sure you want to claim {} rewards?", args.amount).red()))
                .with_default(true)
                .prompt()
                .unwrap_or(true);

            if confirm_claim {
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
                    "Mine" => {
                        let threads = Text::new("Enter the number of threads:")
                            .with_default("4") // Set the default value to 4
                            .prompt()?;
                        let args = MineArgs { threads: threads.parse()? };
                        mine(args, key, base_url, unsecure_conn).await;
                    },
                    "ProtoMine" => {
                        let threads = Text::new("Enter the number of threads:")
                            .with_default("4") // Set the default value to 4
                            .prompt()?;
                        let args = ProtoMineArgs { threads: threads.parse()? };
                        protomine(args, key, base_url, unsecure_conn).await;
                    },
                    "Sign up" => {
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
                    "Claim Rewards" => {
                        // Display claimable balance before confirming claim
                        balance(&key, base_url.clone(), unsecure_conn).await;
                        println!();

                        // Input validation loop for amount
                        let amount: f64 = loop {
                            let input = Text::new("Enter the amount to claim:").prompt().unwrap_or_default();
                            
                            match input.trim().parse::<f64>() {
                                Ok(valid_amount) => break valid_amount,
                                Err(_) => {
                                    println!("Invalid input. Please enter a valid number.");
                                }
                            }
                        };

                        let confirm_claim = Confirm::new(&format!("{}", format!("Are you sure you want to claim {} rewards?", amount).red()))
                            .with_default(true)
                            .prompt()
                            .unwrap_or(true);

                        if confirm_claim {
                            let args = ClaimArgs { amount };
                            claim::claim(args, key, base_url, unsecure_conn).await;
                            prompt_to_continue();
                        } else {
                            println!("Claim cancelled.");
                            prompt_to_continue();
                        }
                    },
                    "View Balances" => {
                        balance(&key, base_url, unsecure_conn).await;
                        prompt_to_continue();
                    },
                    "Help" => {
                        println!("This is a command line tool to interact with the Ec1ipse Ore Mining Pool. Use the options provided to mine, check balances, claim rewards, or sign up.");
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