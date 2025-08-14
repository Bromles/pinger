use std::{net::IpAddr, sync::Arc, time::Duration};

use clap::Parser;
use file_rotate::TimeFrequency;
use hickory_resolver::TokioResolver;
use humantime::parse_duration;
use ping::Ping;
use tokio::signal;
use tokio::{runtime, task::spawn_blocking};
use tracing::{error, info};
use tracing_subscriber_multi::{
    AnsiStripper, AppendCount, Compression, ContentLimit, DualWriter, FmtSubscriber, RotatingFile,
};

/// Pinger with logging to monitor network activity
#[derive(Parser, Debug)]
#[command(about, long_about = None)]
struct Args {
    /// address to ping
    #[arg(short, long, value_parser = Args::parse_address)]
    address: IpAddr,

    /// interval between pings
    #[arg(short, long, value_parser = parse_duration, default_value = "5s")]
    interval: Duration,
}

impl Args {
    fn parse_address(address_str: &str) -> Result<IpAddr, String> {
        if let Ok(addr) = address_str.parse::<IpAddr>() {
            return Ok(addr);
        }

        let resolver = TokioResolver::builder_tokio()
            .map_err(|err| err.to_string())?
            .build();

        let res = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| err.to_string())?
            .block_on(resolver.lookup_ip(address_str))
            .map_err(|err| err.to_string())?;

        let address_opt = res.iter().next();

        let Some(address) = address_opt else {
            return Err("No IP address found".to_string());
        };

        Ok(address)
    }
}

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with_ansi(true)
        .with_writer(std::sync::Mutex::new(DualWriter::new(
            std::io::stderr(),
            AnsiStripper::new(RotatingFile::new(
                "pinger.log",
                AppendCount::new(3),
                ContentLimit::Time(TimeFrequency::Hourly),
                Compression::OnRotate(0),
            )),
        )))
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("failed to initialise logger");

    let args = Args::parse();

    let runtime = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        tokio::select! {
            res = run(&args) => {
                if let Err(err) = res {
                    error!("Error: {}", err);
                }
            },
            _ = shutdown_signal() => {
                info!("Shutting down");
            }
        }
    });
}

async fn run(args: &Args) -> Result<(), String> {
    let mut interval = tokio::time::interval(args.interval);
    let addr = Arc::new(args.address);

    loop {
        interval.tick().await;

        let addr_clone = addr.clone();

        let res = spawn_blocking(move || {
            let pinger = Ping::new(*addr_clone);
            return pinger.send();
        })
        .await
        .map_err(|err| err.to_string())?;

        if res.is_err() {
            error!("Failed to ping to {}", addr);
        }

        info!("Sent ping to {}", addr);
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
