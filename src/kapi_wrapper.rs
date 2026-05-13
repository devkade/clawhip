use std::time::Duration;

use tokio::time::sleep;

use crate::Result;
use crate::cli::KapiWatchArgs;
use crate::client::DaemonClient;
use crate::config::{AppConfig, KapiRepoMonitor};
use crate::source::kapi::poll_monitor_events;

pub async fn watch(args: KapiWatchArgs, config: &AppConfig) -> Result<()> {
    let monitor = KapiRepoMonitor {
        from: args.from.clone(),
        channel: args.channel.clone(),
        mention: args.mention.clone(),
        stale_minutes: args.stale_minutes,
        format: args.format.clone(),
        cursor_path: args
            .cursor_path
            .as_ref()
            .map(|path| path.display().to_string()),
        kapi_bin: args.kapi_bin.clone(),
    };

    loop {
        match run_one_poll(&monitor, config).await {
            Ok(()) => {}
            Err(error) if args.once => return Err(error),
            Err(error) => eprintln!("clawhip kapi watch failed for {}: {error}", monitor.from),
        }

        if args.once {
            return Ok(());
        }
        sleep(Duration::from_secs(args.poll_interval_secs.max(1))).await;
    }
}

async fn run_one_poll(monitor: &KapiRepoMonitor, config: &AppConfig) -> Result<()> {
    let client = DaemonClient::from_config(config);
    let (events, _outcome) = poll_monitor_events(monitor).await?;
    for event in events {
        client.send_event(&event).await?;
    }
    Ok(())
}
