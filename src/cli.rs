use std::{ffi::OsString, path::PathBuf, time::Duration};

use clap::{ColorChoice, Parser};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Check HTTP(S) availability from the terminal or CI",
    color = ColorChoice::Never,
    after_help = "Run without arguments in a terminal for guided setup (arrow keys and Enter).\n\
                  Direct commands check once by default. Only final HTTP 2xx responses are healthy.\n\
                  Redirects are followed; response bodies are not consumed.\n\
                  JSON output is one object per completed check (JSON Lines).\n\
                  URL usernames/passwords are rejected; queries/fragments are omitted from output.\n\n\
                  Exit codes: 0 all healthy, 1 unhealthy, 2 usage/runtime error, 130 Ctrl-C/cancel.\n\n\
                  Examples:\n  websitehealth\n  websitehealth https://example.com https://example.org\n  \
                  websitehealth -w -a -i 30 -f websitehealth.urls\n  \
                  websitehealth -j -t 5 https://example.com"
)]
pub struct Cli {
    /// One or more absolute HTTP(S) URLs to check
    #[arg(value_name = "URL")]
    pub urls: Vec<String>,

    /// Choose URLs and settings with interactive menus
    #[arg(short, long)]
    pub setup: bool,

    /// Read URLs from a file (one per line; blank lines and # comments allowed)
    #[arg(short, long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Keep checking until Ctrl-C
    #[arg(short, long)]
    pub watch: bool,

    /// Seconds after each watch round (default: 60; range: 0.001..=86400)
    #[arg(short, long, requires = "watch", value_name = "SECONDS", value_parser = parse_seconds)]
    pub interval: Option<Duration>,

    /// Per-request timeout, including redirects (0.001..=86400 seconds)
    #[arg(short, long, default_value = "10", value_name = "SECONDS", value_parser = parse_seconds)]
    pub timeout: Duration,

    /// Maximum number of simultaneous requests (1..=256)
    #[arg(short, long, default_value_t = 8, value_name = "N", value_parser = clap::value_parser!(u16).range(1..=256))]
    pub concurrency: u16,

    /// Emit one JSON object per completed check to stdout
    #[arg(short, long)]
    pub json: bool,

    /// Alert on initial failure and DOWN/RECOVERED changes (requires --watch)
    #[arg(short, long, requires = "watch")]
    pub alerts: bool,

    /// Also ring the terminal bell on alerts (requires --alerts)
    #[arg(short, long, requires = "alerts")]
    pub beep: bool,
}

pub fn parse_seconds(value: &str) -> Result<Duration, &'static str> {
    let seconds = value
        .parse::<f64>()
        .map_err(|_| "expected a number of seconds")?;
    if !seconds.is_finite() || !(0.001..=86_400.0).contains(&seconds) {
        return Err("seconds must be between 0.001 and 86400");
    }
    Ok(Duration::from_secs_f64(seconds))
}

pub fn safe_parser_error(error: &clap::Error, args: &[OsString]) -> String {
    // Clap normalizes attached/clustered options before rendering errors, so
    // replacing the original argument is insufficient. Never render a parser
    // diagnostic with URL-like input, regardless of how the option was spelled.
    if args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg.contains("://") || arg.contains(['@', '?', '#'])
    }) {
        "error: invalid arguments (URL values omitted). Run 'websitehealth --help' for usage.\n"
            .into()
    } else {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_limits_reject_nonfinite_zero_and_extreme_values() {
        for value in [
            "", "no", "NaN", "inf", "-inf", "-1", "0", "0.0001", "86401", "1e300",
        ] {
            assert!(parse_seconds(value).is_err(), "accepted {value}");
        }
        assert_eq!(parse_seconds("0.001"), Ok(Duration::from_millis(1)));
        assert_eq!(parse_seconds("86400"), Ok(Duration::from_secs(86400)));
    }

    #[test]
    fn cli_defaults_to_one_round_and_bounds_concurrency() {
        let cli = Cli::try_parse_from(["websitehealth", "https://example.com"]).unwrap();
        assert!(!cli.watch);
        assert!(!cli.alerts);
        assert_eq!(cli.timeout, Duration::from_secs(10));
        assert_eq!(cli.concurrency, 8);
        for value in ["0", "257", "-1"] {
            assert!(
                Cli::try_parse_from([
                    "websitehealth",
                    "--concurrency",
                    value,
                    "https://example.com"
                ])
                .is_err()
            );
        }
        for args in [vec!["--interval", "1"], vec!["--alerts"], vec!["--beep"]] {
            assert!(Cli::try_parse_from(std::iter::once("websitehealth").chain(args)).is_err());
        }
    }
}
