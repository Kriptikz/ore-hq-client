use claim::ClaimArgs;
use solana_sdk::signature::read_keypair_file;
use clap::{Parser, Subcommand};
use inquire::{Text, Confirm, Select};
use dirs::home_dir;
use std::path::PathBuf;

use signup::signup;

mod signup;
mod protomine;
mod mine;
mod claim;
mod balance;
mod rewards;

// --------------------------------

/// A command line interface tool for pooling power to submit hashes for proportional ORE rewards
#[derive(Parser, Debug)]
#[command(version, author, about, long_about = None)]
struct Args {
    #[arg(long,
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
    #[command(about = "Connect to pool and start mining. (Default Implementation)")]
    Mine(mine::MineArgs),
    #[command(about = "Connect to pool and start mining. (Protomine Implementation)")]
    Protomine(protomine::MineArgs),
    #[command(about = "Transfer sol to the pool authority to sign up.")]
    Signup,
    #[command(about = "Claim rewards.")]
    Claim(ClaimArgs),
    #[command(about = "Display claimable rewards.")]
    Rewards,
    #[command(about = "Display current ore token balance.")]
    Balance,
}

// --------------------------------

#[tokio::main]
async fn main() {
    loop {
        if let Err(_) = run_menu().await {
            println!("An error occurred, returning to the main menu...");
        }
    }
}

async fn run_menu() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let options = vec![
        "Mine",
        "ProtoMine",
        "Sign up",
        "Claim Rewards",
        "Check Reward Balance",
        "Check Wallet Balance",
        "Help",
        "Exit",
    ];

    println!(); // Blank line before the prompt

    let selection = match &args.command {
        Some(_) => None, // Execute the command passed as argument
        None => Select::new("Welcome to Ore HQ Client, what would you like to do?", options).prompt().ok(),
    };

    if selection == Some("Exit") {
        std::process::exit(0);
    }

    if selection == Some("Help") {
        println!("This is a command line tool to interact with the ORE mining pool. Use the options provided to mine, check balances, claim rewards, or sign up.");
        return Ok(());
    }

    let base_url = args.url;
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
            .prompt()
            .unwrap_or(false);

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
            mine::mine(args, key, base_url, unsecure_conn).await;
        },
        Some(Commands::Protomine(args)) => {
            protomine::mine(args, key, base_url, unsecure_conn).await;
        },
        Some(Commands::Signup) => {
            let confirm_signup = Confirm::new("Are you sure you want to sign up to the pool?")
                .prompt()
                .unwrap_or(false);

            if confirm_signup {
                signup(base_url, key, unsecure_conn).await;
            } else {
                println!("Sign up cancelled.");
            }
        },
        Some(Commands::Claim(args)) => {
            let confirm_claim = Confirm::new(&format!("Are you sure you want to claim {} rewards?", args.amount))
                .prompt()
                .unwrap_or(false);

            if confirm_claim {
                claim::claim(args, key, base_url, unsecure_conn).await;
            } else {
                println!("Claim cancelled.");
            }
        },
        Some(Commands::Rewards) => {
            rewards::rewards(key, base_url, unsecure_conn).await;
        },
        Some(Commands::Balance) => {
            balance::balance(key, base_url, unsecure_conn).await;
        },
        None => {
            if let Some(choice) = selection {
                match choice {
                    "Mine" => {
                        let cores = Text::new("Enter the number of cores:").prompt()?;
                        let args = mine::MineArgs { cores: cores.parse()? };
                        mine::mine(args, key, base_url, unsecure_conn).await;
                    },
                    "ProtoMine" => {
                        let cores = Text::new("Enter the number of cores:").prompt()?;
                        let args = protomine::MineArgs { cores: cores.parse()? };
                        protomine::mine(args, key, base_url, unsecure_conn).await;
                    },
                    "Sign up" => {
                        let confirm_signup = Confirm::new("Are you sure you want to sign up to the pool?")
                            .prompt()
                            .unwrap_or(false);

                        if confirm_signup {
                            signup(base_url, key, unsecure_conn).await;
                        } else {
                            println!("Sign up cancelled.");
                        }
                    },
                    "Claim Rewards" => {
                        let amount = Text::new("Enter the amount to claim:").prompt()?;
                        let confirm_claim = Confirm::new(&format!("Are you sure you want to claim {} rewards?", amount))
                            .prompt()
                            .unwrap_or(false);

                        if confirm_claim {
                            let args = ClaimArgs { amount: amount.parse()? };
                            claim::claim(args, key, base_url, unsecure_conn).await;
                        } else {
                            println!("Claim cancelled.");
                        }
                    },
                    "Check Reward Balance" => {
                        rewards::rewards(key, base_url, unsecure_conn).await;
                    },
                    "Check Wallet Balance" => {
                        balance::balance(key, base_url, unsecure_conn).await;
                    },
                    "Help" => {
                        println!("This is a command line tool to interact with the ORE mining pool. Use the options provided to mine, check balances, claim rewards, or sign up.");
                    },
                    _ => println!("Unknown selection."),
                }
            }
        },
    }

    Ok(())
}
