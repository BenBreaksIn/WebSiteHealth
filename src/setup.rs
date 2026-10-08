use std::{
    fs,
    io::{self, IsTerminal, Read, Write},
    path::Path,
    time::Duration,
};

use dialoguer::{Input, MultiSelect, Select, console::Term};

use crate::{
    cli::{Cli, parse_seconds},
    monitor::Target,
};

const SAVED_URLS: &str = "websitehealth.urls";
const MAX_FILE_BYTES: u64 = 1_048_576;

// Prompt input runs on a worker thread. An external SIGINT can end the async
// task while that thread is reading a key; restore the user's terminal from
// the main thread as well as dialoguer's normal per-key cleanup.
pub struct TerminalGuard {
    terminal: bool,
    #[cfg(unix)]
    original: Option<libc::termios>,
}

impl TerminalGuard {
    pub fn capture(guided: bool) -> Self {
        let terminal = guided && io::stdin().is_terminal() && io::stderr().is_terminal();
        Self {
            terminal,
            #[cfg(unix)]
            original: if terminal {
                let mut state = std::mem::MaybeUninit::uninit();
                // SAFETY: tcgetattr writes a termios into the valid output
                // pointer. Only assume initialization after a successful call.
                if unsafe { libc::tcgetattr(libc::STDIN_FILENO, state.as_mut_ptr()) } == 0 {
                    Some(unsafe { state.assume_init() })
                } else {
                    None
                }
            } else {
                None
            },
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(original) = &self.original {
            // SAFETY: original is an initialized termios captured above; the
            // descriptor is the same process stdin used by the prompts.
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, original);
            }
        }
        if self.terminal {
            let _ = Term::stderr().show_cursor();
        }
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn canceled() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "setup canceled")
}

pub fn validate_urls(urls: &[String]) -> io::Result<Vec<Target>> {
    if urls.is_empty() {
        return Err(invalid(
            "provide at least one URL or --file PATH; run without arguments in a terminal for guided setup",
        ));
    }
    urls.iter()
        .enumerate()
        .map(|(index, url)| {
            Target::parse(url).map_err(|message| invalid(format!("URL #{}: {message}", index + 1)))
        })
        .collect()
}

pub fn read_urls(path: &Path) -> io::Result<Vec<String>> {
    if !fs::metadata(path)
        .map_err(|error| io::Error::new(error.kind(), "could not open the URL file"))?
        .is_file()
    {
        return Err(invalid("the URL file must be a regular text file"));
    }
    let file = fs::File::open(path)
        .map_err(|error| io::Error::new(error.kind(), "could not open the URL file"))?;
    if !file.metadata()?.is_file() {
        return Err(invalid("the URL file must be a regular text file"));
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| {
            io::Error::new(error.kind(), "could not read the URL file as UTF-8 text")
        })?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid("the URL file must be no larger than 1 MiB"));
    }
    let urls: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();
    validate_urls(&urls)?;
    Ok(urls)
}

pub fn configure(mut cli: Cli) -> io::Result<Cli> {
    let terminal = io::stdin().is_terminal() && io::stderr().is_terminal();
    let guided = cli.setup || (terminal && cli.urls.is_empty() && cli.file.is_none());
    if guided && !terminal {
        return Err(invalid(
            "guided setup needs terminal stdin and stderr; use URLs or --file PATH in scripts",
        ));
    }
    if let Some(path) = &cli.file {
        let mut from_file = read_urls(path)?;
        from_file.extend(cli.urls);
        cli.urls = from_file;
    }
    if guided {
        cli = wizard(cli)?;
    }
    validate_urls(&cli.urls)?;
    Ok(cli)
}

fn choose(term: &Term, prompt: &str, items: &[&str], default: usize) -> io::Result<usize> {
    Select::new()
        .with_prompt(prompt)
        .items(items)
        .default(default)
        .interact_on_opt(term)?
        .ok_or_else(canceled)
}

// Text is entered once; menus and subsequent summaries only show sanitized URLs.
fn enter_urls(term: &Term) -> io::Result<Vec<String>> {
    loop {
        let input: String = Input::new()
            .with_prompt("Websites (space-separated; https:// added if omitted)")
            .report(false)
            .interact_text_on(term)?;
        match normalize_input(&input) {
            Ok(urls) => return Ok(urls),
            Err(error) => term.write_line(&format!("{error}. Please try again."))?,
        }
    }
}

fn normalize_input(input: &str) -> io::Result<Vec<String>> {
    let urls: Vec<_> = input
        .split_whitespace()
        .map(|value| {
            if value.contains("://") {
                value.to_owned()
            } else {
                format!("https://{value}")
            }
        })
        .collect();
    validate_urls(&urls)?;
    Ok(urls)
}

fn duration_input(term: &Term, prompt: &str, default: Duration) -> io::Result<Duration> {
    let value: String = Input::new()
        .with_prompt(prompt)
        .default(default.as_secs_f64().to_string())
        .validate_with(|value: &String| parse_seconds(value).map(|_| ()))
        .interact_text_on(term)?;
    parse_seconds(&value).map_err(invalid)
}

fn wizard(mut cli: Cli) -> io::Result<Cli> {
    let term = Term::stderr();
    // Select hides the cursor. Restore it even if read_key returns Interrupted.
    struct Cursor(Term);
    impl Drop for Cursor {
        fn drop(&mut self) {
            let _ = self.0.show_cursor();
        }
    }
    let _cursor = Cursor(term.clone());
    term.write_line("WebSiteHealth — guided setup")?;
    term.write_line(
        "Use arrow keys + Enter. Space toggles URLs. Esc cancels menus; Ctrl-C cancels any prompt.",
    )?;

    let saved_path = Path::new(SAVED_URLS);
    if cli.urls.is_empty() && saved_path.exists() {
        cli.urls = read_urls(saved_path)?;
    }
    let mut changed = cli.urls.is_empty();
    if !cli.urls.is_empty() {
        validate_urls(&cli.urls)?;
        match choose(
            &term,
            "URLs",
            &[
                "Use the available URL list",
                "Enter a new list",
                "Add websites to this list",
            ],
            0,
        )? {
            1 => {
                cli.urls = enter_urls(&term)?;
                changed = true;
            }
            2 => {
                cli.urls.extend(enter_urls(&term)?);
                changed = true;
            }
            _ => {}
        }
    } else {
        cli.urls = enter_urls(&term)?;
    }
    let all_urls = cli.urls.clone();
    let targets = validate_urls(&all_urls)?;
    if targets.len() > 1 {
        let labels: Vec<_> = targets
            .iter()
            .enumerate()
            .map(|(i, target)| format!("#{} {}", i + 1, target.display_url()))
            .collect();
        loop {
            let selection = MultiSelect::new()
                .with_prompt("Select websites to check (Space toggles, Enter accepts)")
                .items(&labels)
                .defaults(&vec![true; labels.len()])
                .interact_on_opt(&term)?
                .ok_or_else(canceled)?;
            if !selection.is_empty() {
                cli.urls = selection.into_iter().map(|i| all_urls[i].clone()).collect();
                break;
            }
            term.write_line("Select at least one website.")?;
        }
    }
    cli.watch = choose(
        &term,
        "Check mode",
        &["Keep monitoring (Ctrl-C to stop)", "Check once"],
        0,
    )? == 0;
    if cli.watch {
        let interval = cli.interval.unwrap_or(Duration::from_secs(60));
        let choices = [15, 30, 60, 300];
        let default = choices
            .iter()
            .position(|&s| Duration::from_secs(s) == interval)
            .unwrap_or(4);
        cli.interval = Some(
            match choose(
                &term,
                "Wait between rounds",
                &[
                    "15 seconds",
                    "30 seconds",
                    "60 seconds",
                    "5 minutes",
                    "Custom interval",
                ],
                default,
            )? {
                index @ 0..=3 => Duration::from_secs(choices[index]),
                _ => duration_input(&term, "Interval in seconds", interval)?,
            },
        );
        let alerts = choose(
            &term,
            "Local alerts",
            &[
                "Messages on DOWN / RECOVERED",
                "Messages + terminal beep",
                "Off",
            ],
            usize::from(cli.beep),
        )?;
        cli.alerts = alerts != 2;
        cli.beep = alerts == 1;
    } else {
        cli.interval = None;
        cli.alerts = false;
        cli.beep = false;
    }
    cli.json = choose(
        &term,
        "Output",
        &["Readable text", "JSON Lines (for scripts and logs)"],
        usize::from(cli.json),
    )? == 1;
    if choose(
        &term,
        "Request settings",
        &[
            "Use current timeout and concurrency",
            "Customize timeout and concurrency",
        ],
        0,
    )? == 1
    {
        cli.timeout = duration_input(&term, "Timeout in seconds", cli.timeout)?;
        cli.concurrency = Input::<u16>::new()
            .with_prompt("Maximum simultaneous checks")
            .default(cli.concurrency)
            .validate_with(|value: &u16| {
                if (1..=256).contains(value) {
                    Ok(())
                } else {
                    Err("choose 1 through 256")
                }
            })
            .interact_text_on(&term)?;
    }
    // Saving is explicit and never overwrites an existing file. Keep query
    // values so saved checks address the same endpoints, and disclose this.
    if changed
        && !saved_path.exists()
        && choose(
            &term,
            "Remember this URL list for next time?",
            &[
                "Do not save",
                "Save to websitehealth.urls (includes query values)",
            ],
            0,
        )? == 1
    {
        save_urls(saved_path, &all_urls)?;
        term.write_line("Saved websitehealth.urls in this folder. Keep it private if URLs contain query tokens.")?;
    }
    term.write_line(&format!(
        "Starting {} URL(s): timeout {}s, concurrency {}.{}",
        cli.urls.len(),
        cli.timeout.as_secs_f64(),
        cli.concurrency,
        if cli.watch {
            " Monitoring until Ctrl-C."
        } else {
            ""
        }
    ))?;
    Ok(cli)
}

pub fn save_urls(path: &Path, urls: &[String]) -> io::Result<()> {
    let targets = validate_urls(urls)?;
    let text = targets
        .iter()
        .map(Target::request_url)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    // create_new also refuses symlinks and protects another process's new file.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            "could not create the saved URL list; existing files are never replaced",
        )
    })?;
    file.write_all(text.as_bytes())?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guided_input_accepts_bare_hosts_and_validates_every_url() {
        assert_eq!(
            normalize_input("example.com http://localhost:8080/health").unwrap(),
            ["https://example.com", "http://localhost:8080/health"]
        );
        assert!(normalize_input("https://good.invalid ftp://bad.invalid").is_err());
        assert!(normalize_input("user:secret@example.com").is_err());
        assert!(normalize_input(" ").is_err());
    }

    #[test]
    fn saved_lists_preserve_endpoints_and_never_overwrite() {
        let path =
            std::env::temp_dir().join(format!("websitehealth-save-{}.urls", std::process::id()));
        let urls = vec!["https://example.com/health?token=secret#secret".into()];
        save_urls(&path, &urls).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "https://example.com/health?token=secret\n"
        );
        assert_eq!(
            read_urls(&path).unwrap(),
            ["https://example.com/health?token=secret"]
        );
        assert_eq!(
            save_urls(&path, &urls).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        fs::remove_file(path).unwrap();
    }
}
