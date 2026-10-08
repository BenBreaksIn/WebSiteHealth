mod cli;
mod monitor;
mod output;
mod setup;

use std::{
    io::{self, IsTerminal},
    process::ExitCode,
    time::Duration,
};

use clap::Parser;
use cli::Cli;
use monitor::Target;
use reqwest::Client;
use tokio::{io::AsyncWrite, task::JoinSet};

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("error: could not initialize the async runtime");
            return ExitCode::from(2);
        }
    };
    let exit = runtime.block_on(async {
        tokio::select! {
            // Also handles interruption while a setup prompt or output blocks.
            biased;
            signal = tokio::signal::ctrl_c() => {
                if signal.is_ok() { ExitCode::from(130) } else { ExitCode::from(2) }
            }
            exit = run_cli() => exit,
        }
    });
    // OS DNS lookups and pipe writes may outlive their canceled async requests.
    // Neither may delay process exit after a timeout or Ctrl-C.
    runtime.shutdown_timeout(Duration::ZERO);
    exit
}

async fn run_cli() -> ExitCode {
    match execute().await {
        Ok(exit) => exit,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => ExitCode::from(130),
        Err(error) => {
            if error.kind() != io::ErrorKind::BrokenPipe
                && let Ok(mut stderr) = output::stderr()
            {
                let _ = output::write(&mut stderr, format!("error: {error}\n").as_bytes()).await;
            }
            ExitCode::from(2)
        }
    }
}

async fn execute() -> io::Result<ExitCode> {
    let args: Vec<_> = std::env::args_os().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) if error.use_stderr() => {
            output::write(
                &mut output::stderr()?,
                cli::safe_parser_error(&error, &args).as_bytes(),
            )
            .await?;
            return Ok(ExitCode::from(2));
        }
        Err(error) => {
            output::write(&mut output::stdout()?, error.to_string().as_bytes()).await?;
            return Ok(ExitCode::SUCCESS);
        }
    };
    let _terminal =
        setup::TerminalGuard::capture(cli.setup || (cli.urls.is_empty() && cli.file.is_none()));
    let cli = tokio::task::spawn_blocking(move || setup::configure(cli))
        .await
        .map_err(|_| io::Error::other("setup failed"))??;
    let targets = setup::validate_urls(&cli.urls)?;
    let client = monitor::client(cli.timeout)
        .map_err(|_| io::Error::other("could not initialize the HTTP/TLS client"))?;
    let mut stdout = output::stdout()?;
    let mut stderr = output::stderr()?;
    let healthy = run(&cli, &targets, &client, &mut stdout, &mut stderr).await?;
    Ok(if healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

async fn run(
    cli: &Cli,
    targets: &[Target],
    client: &Client,
    stdout: &mut (impl AsyncWrite + Unpin),
    stderr: &mut (impl AsyncWrite + Unpin),
) -> io::Result<bool> {
    let mut alerts = cli
        .alerts
        .then(|| output::Alerts::new(targets.len(), cli.beep && io::stderr().is_terminal()));
    loop {
        let healthy = check_round(targets, client, cli, stdout, stderr, &mut alerts).await?;
        if !cli.watch {
            return Ok(healthy);
        }
        tokio::time::sleep(cli.interval.unwrap_or(Duration::from_secs(60))).await;
    }
}

async fn check_round(
    targets: &[Target],
    client: &Client,
    cli: &Cli,
    stdout: &mut (impl AsyncWrite + Unpin),
    stderr: &mut (impl AsyncWrite + Unpin),
    alerts: &mut Option<output::Alerts>,
) -> io::Result<bool> {
    // At most concurrency checks plus one result awaiting output are retained.
    // Slow readers apply backpressure; the runtime remains free to handle
    // in-flight request deadlines and Ctrl-C while output is blocked.
    let mut pending = targets.iter().enumerate();
    let mut checks = JoinSet::new();
    for (index, target) in pending.by_ref().take(usize::from(cli.concurrency)) {
        checks.spawn(monitor::check(client.clone(), target.clone(), index));
    }
    let mut healthy = true;
    while let Some(result) = checks.join_next().await {
        let result = result.map_err(|_| io::Error::other("a check task failed"))?;
        healthy &= result.healthy;
        if let Some(alerts) = alerts
            && let Some(message) = alerts.observe(&result)
        {
            output::write(stderr, message.as_bytes()).await?;
        }
        output::write(stdout, &output::record(&result, cli.json)?).await?;
        if let Some((index, target)) = pending.next() {
            checks.spawn(monitor::check(client.clone(), target.clone(), index));
        }
    }
    Ok(healthy)
}
