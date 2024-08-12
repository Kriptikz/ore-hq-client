use solana_sdk::signature::read_keypair_file;
use clap::{Parser, Subcommand};

use mine::MineArgs;
use signup::signup;

mod signup;
mod mine;

// --------------------------------

/// A command line interface tool for pooling power to submit hashes for proportional ORE rewards
#[derive(Parser, Debug)]
#[command(version, author, about, long_about = None)]
struct Args {
    #[arg(long,
        value_name = "SERVER_URL",
        help = "URL of the server to connect to",
        default_value = "domainexpansion.tech",
    )]
    url: String,

    #[arg(
        long,
        value_name = "KEYPAIR_PATH",
        help = "Filepath to keypair to use",
    )]
    keypair: String,

    #[command(subcommand)]
    command: Commands
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command(about = "Connect to pool and start mining.")]
    Mine(MineArgs),
    #[command(about = "Transfer sol to the pool authority to sign up.")]
    Signup,
}

// --------------------------------


#[tokio::main]
async fn main() {
    let args = Args::parse();

    let base_url = args.url;
    let key = read_keypair_file(args.keypair.clone()).expect(&format!("Failed to load keypair from file: {}", args.keypair));
    match args.command {
        Commands::Mine(args) => {
            mine::mine(args, key, base_url).await;
        },
        Commands::Signup => {
            signup(base_url, key).await;
        }
    }


}

