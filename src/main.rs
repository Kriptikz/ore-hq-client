use claim::ClaimArgs;
use delegate_stake::StakeArgs;
use solana_sdk::signature::read_keypair_file;
use clap::{Parser, Subcommand};

use signup::signup;

mod signup;
mod protomine;
mod mine;
mod claim;
mod balance;
mod rewards;
mod delegate_stake;
mod stake_balance;

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
    command: Commands
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
    #[command(about = "Delegate stake for the pool miner.")]
    Stake(StakeArgs),
    #[command(about = "Delegated stake balance.")]
    StakeBalance,
}

// --------------------------------


#[tokio::main]
async fn main() {
    let args = Args::parse();

    let base_url = args.url;
    let unsecure_conn = args.use_http;
    let key = read_keypair_file(args.keypair.clone()).expect(&format!("Failed to load keypair from file: {}", args.keypair));
    match args.command {
        Commands::Mine(args) => {
            mine::mine(args, key, base_url, unsecure_conn).await;
        },
        Commands::Protomine(args) => {
            protomine::mine(args, key, base_url, unsecure_conn).await;
        },
        Commands::Signup => {
            signup(base_url, key, unsecure_conn).await;
        },
        Commands::Claim(args) => {
            claim::claim(args, key, base_url, unsecure_conn).await;
        }
        Commands::Rewards => {
            rewards::rewards(key, base_url, unsecure_conn).await;
        }
        Commands::Balance => {
            balance::balance(key, base_url, unsecure_conn).await;
        }
        Commands::Stake(args) => {
            delegate_stake::delegate_stake(args, key, base_url, unsecure_conn).await;
        }
        Commands::StakeBalance => {
            stake_balance::stake_balance(key, base_url, unsecure_conn).await;
        }
    }


}

