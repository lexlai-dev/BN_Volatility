use tokio::time::{sleep, Duration};
use tracing::{info, error};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

use volatility_monitor::config::MonitorConfig;
use volatility_monitor::indicators::vol::InstantVolatilityIndicator;
use volatility_monitor::run_connection;

/// Custom timer implementation to format log timestamps using the system's local timezone.
/// By default, tracing uses UTC (Zulu time), which can be confusing for local debugging.
struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        let now = chrono::Local::now();
        write!(w, "{}", now.format("%Y-%m-%dT%H:%M:%S%.3f"))
    }
}

#[tokio::main]
async fn main() {
    // Initialize the tracing subscriber.
    // 1. Reads the log level from the RUST_LOG environment variable (defaults to "info").
    // 2. Injects the custom LocalTimer to ensure logs show local time.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_timer(LocalTimer)
        .init();

    // Load configuration immediately at startup.
    // Adopts a "Fail Fast" strategy: if the config is missing or invalid, exit immediately.
    let cfg = match MonitorConfig::load() {
        Ok(c) => c,
        Err(e) => {
            error!("❌ Critical Error: Failed to load configuration: {}", e);
            return;
        }
    };

    // Initialize the volatility calculator with a 30-sample window and 15ms sampling interval.
    // Instantiated outside the loop to potentially preserve state across reconnections.
    let mut vol_calc = InstantVolatilityIndicator::new(30, 15);

    loop {
        info!("🚀 Starting Binance Volatility Monitor...");

        // Run the core connection logic imported from the library.
        if let Err(e) = run_connection(&mut vol_calc, &cfg).await {
            error!("⚠️ Connection lost: {:?}. Retrying in 5s...", e);
        }

        sleep(Duration::from_secs(5)).await;
    }
}