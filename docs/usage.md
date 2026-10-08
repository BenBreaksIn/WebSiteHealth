# Usage reference

[Back to WebSiteHealth](../README.md)

### Guided setup: minimal typing

Run `websitehealth` in a terminal. Enter websites once, separated by spaces;
the guide adds `https://` when you omit it. Use **arrow keys and Enter** to choose
settings and **Space** to select or deselect websites. You can choose:

- Check once or monitor continuously (the guide defaults to continuous).
- A common interval or a custom value.
- DOWN/RECOVERED messages, messages with a beep, or no alerts.
- Readable text or JSON Lines.
- Default request settings or custom timeout and concurrency.

The guide can save the full URL list to `websitehealth.urls` in the current
folder, with your explicit selection. Next time, launch from the same folder and
choose from that list. Selection affects only the current run. Existing files
are never overwritten; edit the text file to update saved URLs. Timeout,
interval, and alert settings are selected per run and are not saved.

Saved URLs **include query values**, so checks keep addressing the same
endpoints. Keep the file private if those values contain tokens; on Unix, new
files are created with owner-only permissions. Fragments are not saved.
The filename is ignored by this repository's Git configuration.

Use `--setup` (or `-s`) to open the guide with URLs or a file already supplied.
Menus require terminal stdin and stderr. Scripts without URLs exit with code `2`
instead of prompting. All prompts go to stderr, leaving stdout available for
results. **Esc** cancels menus; **Ctrl-C** cancels any prompt or running monitor.

### Direct commands and saved lists

```text
websitehealth [OPTIONS] [URL]...
```

| Option | Behavior |
| --- | --- |
| `-s`, `--setup` | Open the interactive guide. Also opens automatically without URLs in a terminal. |
| `-f`, `--file PATH` | Read URLs from a UTF-8 text file, one per line. |
| `-w`, `--watch` | Repeat until Ctrl-C. Direct commands otherwise check once. |
| `-i`, `--interval SECONDS` | Wait after each complete watch round; default `60`. Requires `--watch`. |
| `-t`, `--timeout SECONDS` | Timeout for each request, including redirects; default `10`. |
| `-c`, `--concurrency N` | At most `N` requests at once; default `8`, allowed range `1–256`. |
| `-j`, `--json` | Write one JSON object per result (JSON Lines). |
| `-a`, `--alerts` | Report initial failures and DOWN/RECOVERED transitions to stderr. Requires `--watch`. |
| `-b`, `--beep` | Also ring the terminal bell on alerts. Requires `--alerts`. |
| `-h`, `--help` | Show help and examples. |
| `-V`, `--version` | Show the version. |

Timeout and interval values accept fractional seconds from `0.001` through
`86400`. Quote URLs containing shell characters such as `&`.

URL files allow blank lines and whole-line `#` comments, and must be regular
files no larger than 1 MiB. File URLs are checked first in input numbering,
followed by any command-line URLs. All are validated before network checks begin.
Direct commands and URL files require explicit `http://` or `https://` schemes.

```sh
# Use a URL list saved by the guide, with no repeated URL typing.
websitehealth -f websitehealth.urls
websitehealth -w -a -b -i 30 -f websitehealth.urls

# Select a subset from an existing list using the guide.
websitehealth -s -f websitehealth.urls
```

An example URL file:

```text
# Public sites
https://example.com
https://example.org
```

Results appear as checks complete, so a slow URL does not hold up faster results
when concurrency permits. Watch rounds do not overlap: the interval begins after
all URLs in the round have completed or timed out. A round with failed checks
does not stop watch mode.

### Local alerts

Alerts are opt-in for direct commands. The guide offers them by default in watch
mode. A URL that starts unhealthy produces one `ALERT DOWN` message. Subsequent
messages appear only when that URL changes between unhealthy and healthy
(`ALERT RECOVERED`). Repeated failures do not repeat the alert, and an initially
healthy URL produces no alert. Each input is tracked independently.

Alert text goes to stderr; normal text/JSON results continue on stdout for every
check. With `--beep`, a bell character is emitted only when stderr is a terminal.
Whether it makes a sound depends on your terminal's bell settings. There is no
external notification service, background daemon, or monitoring after the
process exits. Alert history resets when the CLI restarts.

Example text output (timestamps and timings vary):

```text
2026-10-07T19:00:00.123Z UP   #1 https://example.com/ | HTTP 200 | 142 ms
2026-10-07T19:00:00.200Z DOWN #2 http://localhost:8080/health | HTTP 503 | 8 ms
```

### Check semantics

- Each check sends a `GET` with a stable `websitehealth/<version>` User-Agent.
- Only a final HTTP **200–299** response is healthy. Other statuses, network
  errors, timeouts and redirect errors are unhealthy.
- Up to ten HTTP(S) redirects are followed. A redirect with no usable `Location`
  remains a non-2xx result. Redirects containing URL credentials are rejected.
- Response time covers the request through receipt of the final response
  headers, including DNS, connection setup, TLS and redirects as needed. It
  excludes time waiting for a concurrency slot. Response bodies are not consumed;
  this is an availability check, not a page-load or content-integrity test.
- Timestamps are completion times in UTC. The `#` number identifies the URL's
  one-based position in the input list; duplicate URLs are checked separately.
- HTTPS uses normal platform certificate and hostname verification through
  Rustls. There is no insecure TLS bypass. Standard proxy environment variables
  (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`) are honored.
- URLs with usernames or passwords are rejected before checking any targets.
  Query strings are sent to the server but omitted from results, alerts, and
  errors; fragments are neither sent nor included in results. Hosts and paths
  remain visible. Interactive URL entry is visible as you type; later selection
  menus show redacted URLs. Redaction does not hide command arguments from shell
  history or process listings, or query values explicitly saved to a URL file.

### JSON and scripting

Both single checks and watch mode produce **JSON Lines**, rather than one JSON
array. Each completed check writes and flushes one object to stdout. Diagnostics
go to stderr. There are no banners or summaries mixed into JSON output.

```sh
websitehealth --json https://example.com https://example.org > checks.jsonl
websitehealth --watch --interval 60 --json https://example.com >> uptime.jsonl
```

Example record:

```json
{"target":1,"timestamp":"2026-10-07T19:00:00.123Z","url":"https://example.com/","healthy":true,"status":200,"error":null,"response_time_ms":142}
```

`status` is the final HTTP status, or `null` when the request fails. `error` is
`null` for HTTP responses, including HTTP failures; otherwise it is a concise
network error such as `request timed out`. `response_time_ms` is an integer.
Use `target` to distinguish inputs whose query strings differ but whose displayed
URLs are identical.

| Exit code | Meaning |
| --- | --- |
| `0` | Every URL was healthy in a single check; also used for help/version. |
| `1` | At least one URL was unhealthy in a single check. All results are still emitted. |
| `2` | Invalid arguments, initialization failure, or an output/runtime error. |
| `130` | Interrupted with Ctrl-C, or canceled in guided setup. |

Ctrl-C cancels pending work, including while stdout or the alert pipe is full.
Already-completed records remain; interrupting a blocked write may leave the
last record incomplete. A slow reader pauses new scheduling to keep memory
bounded; it does not block the runtime's signal handling or in-flight request
deadlines. A closed output pipe exits with `2` without a panic.

For CI, invoke the CLI directly and let its exit status determine the job result:

```sh
websitehealth --timeout 5 https://example.com
```

