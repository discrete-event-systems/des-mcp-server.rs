//! Shared helpers: subprocess running, output shaping, and input validation.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};

pub const MAX_OUTPUT_CHARS: usize = 40_000;

/// The DES repos this server manages. They currently live under the org root
/// (`~/codes/ores` by default — a directory shared with many NON-DES repos),
/// so tools operate on this explicit allowlist rather than every git repo in
/// the root. Repos will migrate into github.com/discrete-event-systems over
/// time; keep this list in sync when they do.
pub const DES_REPOS: &[&str] = &[
    "discrete-event-system.rs", // current Rust engine (canonical)
    "old-outmoded-engine.rs",   // legacy Rust engine (read-only reference)
    "des-engine",               // TypeScript engine (predecessor of the Rust port)
    "des",                      // research material (uta-phd)
];

pub fn org_root() -> PathBuf {
    if let Ok(root) = std::env::var("DES_ROOT") {
        return PathBuf::from(root);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/maca5".into());
    PathBuf::from(home).join("codes/ores")
}

pub fn truncate_output(mut s: String) -> String {
    if s.len() > MAX_OUTPUT_CHARS {
        let mut cut = MAX_OUTPUT_CHARS;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        let dropped = s.len() - cut;
        s.truncate(cut);
        s.push_str(&format!("\n…[output truncated, {dropped} bytes dropped]"));
    }
    s
}

pub const MAX_COMMAND_STDOUT_BYTES: usize = 1024 * 1024;
pub const MAX_COMMAND_STDERR_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandOutputLimits {
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

/// Run a command with a timeout and pre-buffer stdout/stderr ceilings.
pub async fn run_cmd(
    dir: Option<&Path>,
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(bool, String), String> {
    run_cmd_with_limits(
        dir,
        program,
        args,
        CommandOutputLimits {
            timeout,
            max_stdout_bytes: MAX_COMMAND_STDOUT_BYTES,
            max_stderr_bytes: MAX_COMMAND_STDERR_BYTES,
        },
    )
    .await
}

/// Concurrently drain both child pipes under explicit byte and time limits.
/// The child is killed and reaped on timeout or the first over-limit byte.
pub async fn run_cmd_with_limits(
    dir: Option<&Path>,
    program: &str,
    args: &[&str],
    limits: CommandOutputLimits,
) -> Result<(bool, String), String> {
    const MAX_CONFIGURED_CAPTURE: usize = 16 * 1024 * 1024;
    if limits.timeout.is_zero()
        || !(1..=MAX_CONFIGURED_CAPTURE).contains(&limits.max_stdout_bytes)
        || !(1..=MAX_CONFIGURED_CAPTURE).contains(&limits.max_stderr_bytes)
    {
        return Err("invalid subprocess output limits".to_string());
    }

    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("`{program}` not found on PATH: {error}")
        } else {
            format!("failed to spawn {program}: {error}")
        }
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "subprocess stdout pipe is unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "subprocess stderr pipe is unavailable".to_string())?;

    let operation = async {
        let wait = async {
            child
                .wait()
                .await
                .map_err(|error| format!("{program} failed: {error}"))
        };
        let stdout = read_pipe_bounded(stdout, limits.max_stdout_bytes, "stdout");
        let stderr = read_pipe_bounded(stderr, limits.max_stderr_bytes, "stderr");
        tokio::try_join!(wait, stdout, stderr)
    };

    let result = tokio::time::timeout(limits.timeout, operation).await;
    let (status, stdout, stderr) = match result {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!("`{program}` timed out after {:?}", limits.timeout));
        }
    };

    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    let stderr = String::from_utf8_lossy(&stderr);
    if !stderr.trim().is_empty() {
        text.push_str(
            "
--- stderr ---
",
        );
        text.push_str(&stderr);
    }
    Ok((status.success(), text))
}

async fn read_pipe_bounded<R>(
    mut reader: R,
    limit: usize,
    stream_name: &str,
) -> Result<Vec<u8>, String>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader
            .read(&mut chunk)
            .await
            .map_err(|error| format!("reading subprocess {stream_name} failed: {error}"))?;
        if read == 0 {
            return Ok(output);
        }
        let next = output
            .len()
            .checked_add(read)
            .ok_or_else(|| format!("subprocess {stream_name} length overflow"))?;
        if next > limit {
            return Err(format!(
                "subprocess {stream_name} exceeded the {limit}-byte limit"
            ));
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

pub async fn git(dir: &Path, args: &[&str]) -> Result<(bool, String), String> {
    run_cmd(Some(dir), "git", args, Duration::from_secs(30)).await
}

/// Validate a name used as a single path segment (repo name).
pub fn safe_segment(name: &str, what: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.starts_with('.')
    {
        return Err(format!("invalid {what} name: {name:?}"));
    }
    Ok(())
}

/// Validate a token passed through to a CLI (git ref, cargo test filter, …):
/// conservative charset and never flag-shaped.
pub fn safe_token(name: &str, what: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':' | '/' | '@'));
    if !ok {
        return Err(format!("invalid {what}: {name:?}"));
    }
    Ok(())
}

/// Validate a DNS hostname / domain name.
pub fn safe_hostname(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 253
        && !name.starts_with('-')
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'));
    if !ok {
        return Err(format!("invalid hostname: {name:?}"));
    }
    Ok(())
}

/// Validate a value interpolated into a PostgREST query-string filter
/// (session ids, env names, log levels). UUID/slug charset only.
pub fn safe_pg_filter_value(value: &str, what: &str) -> Result<(), String> {
    let ok = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '@'));
    if !ok {
        return Err(format!("invalid {what}: {value:?}"));
    }
    Ok(())
}

/// Truncate `s` to at most `max_bytes`, snapping the cut DOWN to a UTF-8 char
/// boundary, and append an ellipsis if anything was dropped. Use this for the
/// inline truncation of any data- or network-influenced string (JSON spec
/// fields, upstream API bodies): a raw `&s[..max]` panics when a multi-byte
/// char straddles the cut, turning a malformed/unicode input into a crash.
pub fn truncate_field(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut cut = max_bytes;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &s[..cut])
}

/// Extract per-target `test result:` summary lines from cargo test output.
pub fn parse_test_summaries(output: &str) -> Vec<&str> {
    output
        .lines()
        .filter(|l| l.trim_start().starts_with("test result:"))
        .collect()
}

/// Extract the sorted, deduped set of failing test names from cargo test output.
pub fn parse_failing_tests(output: &str) -> Vec<String> {
    let mut failing: Vec<String> = output
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("test ")
                .and_then(|rest| rest.strip_suffix("... FAILED"))
                .map(|name| name.trim().to_string())
        })
        .collect();
    failing.sort();
    failing.dedup();
    failing
}

/// Parse an openssl `notAfter=` date like "Jun 15 12:00:00 2026 GMT".
pub fn parse_openssl_enddate(s: &str) -> Option<chrono::NaiveDateTime> {
    let s = s.trim().strip_prefix("notAfter=").unwrap_or(s.trim());
    chrono::NaiveDateTime::parse_from_str(s.trim(), "%b %e %H:%M:%S %Y GMT").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_segment_accepts_normal_names() {
        assert!(safe_segment("discrete-event-system.rs", "repo").is_ok());
        assert!(safe_segment("des-engine", "repo").is_ok());
    }

    #[test]
    fn safe_segment_rejects_traversal_and_hidden() {
        for bad in ["", "..", "a/b", "a\\b", ".git", "../etc", "foo/.."] {
            assert!(safe_segment(bad, "repo").is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn safe_token_rejects_flag_injection() {
        assert!(safe_token("main", "branch").is_ok());
        assert!(safe_token("event_queue::tests", "filter").is_ok());
        for bad in ["", "--force", "-n", "a b", "x;y", "$(id)"] {
            assert!(safe_token(bad, "arg").is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn safe_hostname_validation() {
        assert!(safe_hostname("example.com").is_ok());
        assert!(safe_hostname("a-b.c-d.io").is_ok());
        for bad in ["", "-x.com", ".com", "exa mple.com", "a;b.com", "x_y.com"] {
            assert!(safe_hostname(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn safe_pg_filter_value_rejects_query_injection() {
        assert!(safe_pg_filter_value("0b6e6d3c-2f6e-4c39-9e6e-8f0a1b2c3d4e", "session").is_ok());
        assert!(safe_pg_filter_value("production", "env").is_ok());
        assert!(safe_pg_filter_value("user@example.com", "user").is_ok());
        for bad in ["", "a,or.1=1", "x)&select=*", "a b", "eq.x&limit=9999"] {
            assert!(
                safe_pg_filter_value(bad, "value").is_err(),
                "should reject {bad:?}"
            );
        }
    }

    #[test]
    fn truncate_field_snaps_to_char_boundary() {
        // ASCII shorter than the cap passes through unchanged.
        assert_eq!(truncate_field("hello", 100), "hello");
        assert_eq!(truncate_field("hello", 5), "hello");
        // Multi-byte content whose cut lands mid-char must NOT panic; the cut
        // snaps down to a boundary and an ellipsis is appended.
        let s = "€".repeat(30); // 3 bytes each = 90 bytes
        let t = truncate_field(&s, 60); // 60 is not a char boundary here
        assert!(t.ends_with('…'));
        assert!(t.len() <= 60 + '…'.len_utf8());
        // The kept prefix is valid UTF-8 made of whole '€' chars.
        assert!(t.trim_end_matches('…').chars().all(|c| c == '€'));
        // A cap of 0 yields just the ellipsis, still no panic.
        assert_eq!(truncate_field("abc", 0), "…");
    }

    #[test]
    fn truncate_output_marks_dropped_bytes() {
        let s = "x".repeat(MAX_OUTPUT_CHARS + 100);
        let t = truncate_output(s);
        assert!(t.contains("output truncated"));
        assert!(t.len() < MAX_OUTPUT_CHARS + 100);
        assert_eq!(truncate_output("short".into()), "short");
    }

    #[test]
    fn truncate_output_respects_char_boundaries() {
        let s = "é".repeat(MAX_OUTPUT_CHARS); // 2 bytes each
        let t = truncate_output(s);
        assert!(t.contains("output truncated"));
    }

    #[test]
    fn parse_failing_tests_extracts_sorted_set() {
        let out = "\
running 3 tests
test alpha::works ... ok
test zeta::breaks ... FAILED
test beta::breaks ... FAILED

test result: FAILED. 1 passed; 2 failed; 0 ignored
";
        assert_eq!(
            parse_failing_tests(out),
            vec!["beta::breaks", "zeta::breaks"]
        );
        assert_eq!(parse_test_summaries(out).len(), 1);
    }

    #[test]
    fn parse_openssl_enddate_formats() {
        let d = parse_openssl_enddate("notAfter=Jun 15 12:00:00 2026 GMT").unwrap();
        assert_eq!(d.format("%Y-%m-%d").to_string(), "2026-06-15");
        let d = parse_openssl_enddate("notAfter=Jul  3 01:02:03 2027 GMT").unwrap();
        assert_eq!(d.format("%Y-%m-%d").to_string(), "2027-07-03");
        assert!(parse_openssl_enddate("garbage").is_none());
    }

    #[test]
    fn des_repos_list_names_the_canonical_engine() {
        assert!(DES_REPOS.contains(&"discrete-event-system.rs"));
        assert!(DES_REPOS.contains(&"old-outmoded-engine.rs"));
    }
}
