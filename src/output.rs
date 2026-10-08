use std::io;

use tokio::{
    fs::File,
    io::{AsyncWrite, AsyncWriteExt},
};

use crate::monitor::CheckResult;

// Duplicate the OS handles rather than writing through std's global stdout/
// stderr locks. Async file writes leave the runtime free to handle Ctrl-C;
// shutdown_timeout also avoids waiting for a blocked pipe writer at exit.
// A blocked writer holds no global stdio lock that Rust's exit flush needs.
pub fn stdout() -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        Ok(File::from_std(
            std::io::stdout().as_fd().try_clone_to_owned()?.into(),
        ))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        Ok(File::from_std(
            std::io::stdout().as_handle().try_clone_to_owned()?.into(),
        ))
    }
}

pub fn stderr() -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        Ok(File::from_std(
            std::io::stderr().as_fd().try_clone_to_owned()?.into(),
        ))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        Ok(File::from_std(
            std::io::stderr().as_handle().try_clone_to_owned()?.into(),
        ))
    }
}

pub async fn write(output: &mut (impl AsyncWrite + Unpin), bytes: &[u8]) -> io::Result<()> {
    output.write_all(bytes).await?;
    output.flush().await
}

fn detail(result: &CheckResult) -> String {
    match result.status {
        Some(status) => format!("HTTP {status}"),
        None => result.error.unwrap_or("request failed").to_owned(),
    }
}

pub fn record(result: &CheckResult, json: bool) -> io::Result<Vec<u8>> {
    if json {
        let mut bytes = serde_json::to_vec(result)?;
        bytes.push(b'\n');
        Ok(bytes)
    } else {
        let state = if result.healthy { "UP" } else { "DOWN" };
        Ok(format!(
            "{} {state:<4} #{} {} | {} | {} ms\n",
            result.timestamp,
            result.target,
            result.url,
            detail(result),
            result.response_time_ms
        )
        .into_bytes())
    }
}

pub struct Alerts {
    previous: Vec<Option<bool>>,
    beep: bool,
}

impl Alerts {
    pub fn new(targets: usize, beep: bool) -> Self {
        Self {
            previous: vec![None; targets],
            beep,
        }
    }

    pub fn observe(&mut self, result: &CheckResult) -> Option<String> {
        let previous = self.previous[result.target - 1].replace(result.healthy);
        if previous == Some(result.healthy) || (previous.is_none() && result.healthy) {
            return None;
        }
        let state = if result.healthy { "RECOVERED" } else { "DOWN" };
        let bell = if self.beep { "\u{7}" } else { "" };
        Some(format!(
            "{bell}{} ALERT {state} #{} {} | {}\n",
            result.timestamp,
            result.target,
            result.url,
            detail(result)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alerts_track_each_target_and_only_emit_failure_or_transition() {
        let mut alerts = Alerts::new(2, true);
        let mut result = CheckResult {
            target: 1,
            timestamp: "now".into(),
            url: "https://example.com/".into(),
            healthy: true,
            status: Some(200),
            error: None,
            response_time_ms: 1,
        };
        assert!(alerts.observe(&result).is_none());
        result.healthy = false;
        result.status = Some(503);
        assert!(
            alerts
                .observe(&result)
                .unwrap()
                .starts_with("\u{7}now ALERT DOWN #1")
        );
        assert!(alerts.observe(&result).is_none());
        result.target = 2;
        assert!(alerts.observe(&result).unwrap().contains("DOWN #2"));
        result.target = 1;
        result.healthy = true;
        result.status = Some(200);
        assert!(alerts.observe(&result).unwrap().contains("RECOVERED #1"));
        assert!(alerts.observe(&result).is_none());
    }
}
