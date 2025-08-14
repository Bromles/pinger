use std::{net::IpAddr, sync::Arc, time::Duration};

use clap::{Parser, ValueEnum};
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

    /// log file rotation interval
    #[arg(short, long, value_enum, default_value_t)]
    log_rotation: LogRotation,
}

#[derive(ValueEnum, Clone, Default, Debug)]
enum LogRotation {
    /// Rotate every hour.
    Hourly,
    /// Rotate one time a day.
    #[default]
    Daily,
    /// Rotate ones a week.
    Weekly,
    /// Rotate every month.
    Monthly,
    /// Rotate yearly.
    Yearly,
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
    let args = Args::parse();

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
                ContentLimit::Time(map_log_rotation(&args.log_rotation)),
                Compression::OnRotate(0),
            )),
        )))
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("failed to initialise logger");

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

fn map_log_rotation(rotation: &LogRotation) -> TimeFrequency {
    match rotation {
        LogRotation::Hourly => TimeFrequency::Hourly,
        LogRotation::Daily => TimeFrequency::Daily,
        LogRotation::Weekly => TimeFrequency::Weekly,
        LogRotation::Monthly => TimeFrequency::Monthly,
        LogRotation::Yearly => TimeFrequency::Yearly,
    }
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

        match res {
            Ok(_) => {
                info!("Sent ping to {}", addr);
            }
            Err(err) => {
                error!("Failed to ping {}, error: {}", addr, err);
            }
        }
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
