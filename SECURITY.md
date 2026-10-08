# Security policy

Security fixes target the latest release.

Please report suspected vulnerabilities through
[GitHub private vulnerability reporting](https://github.com/BenBreaksIn/WebSiteHealth/security/advisories/new).
Include the version, operating system, reproduction steps, and expected impact.
Use synthetic URLs and credentials; do not share live secrets.

WebSiteHealth intentionally makes requests to the HTTP(S) URLs supplied by the
person running it. Query values are sent to the target server and can be saved
explicitly to a local URL file. Output redaction does not remove URLs from shell
history, process listings, or saved files. See the
[usage reference](docs/usage.md#check-semantics) for the full behavior.
