use std::{net::IpAddr, time::Duration};

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Pinger with logging to monitor network activity
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// address to ping
    #[arg(short, long)]
    address: String,

    /// interval between pings
    #[arg(short, long)]
    interval: u64,

    /// log separation duration
    #[arg(short, long)]
    logs_per_file: u64,
}

impl Args {
    fn convert(&self) -> ParsedArgs {
        let interval = Duration::from_millis(self.interval);
        let logs_per_file = Duration::from_millis(self.logs_per_file);

        ParsedArgs {
            address: self.address.parse().unwrap(),
            interval,
            logs_per_file,
        }
    }
}

#[derive(Debug)]
struct ParsedArgs {
    address: IpAddr,
    interval: Duration,
    logs_per_file: Duration,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .init();

    let args = Args::parse().convert();

    println!("Args: {:?}", args);
}

fn pong(args: &ParsedArgs) -> Result<(), ()> {
    match ping::new(args.address).send() {
        Ok(response) => {
            println!("Pong response: {:?}", response);
            Ok(())
        }
        Err(err) => {
            eprintln!("Error sending ping: {}", err);
            Err(())
        }
    }
}
