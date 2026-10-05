#![cfg(unix)]

use std::time::{Duration, Instant};

use des_mcp_server::util::{CommandOutputLimits, run_cmd_with_limits};

fn limits(stdout: usize, stderr: usize, timeout: Duration) -> CommandOutputLimits {
    CommandOutputLimits {
        timeout,
        max_stdout_bytes: stdout,
        max_stderr_bytes: stderr,
    }
}

#[tokio::test]
async fn concurrently_captures_bounded_stdout_and_stderr() {
    let (ok, output) = run_cmd_with_limits(
        None,
        "sh",
        &["-c", "printf stdout-value; printf stderr-value >&2"],
        limits(128, 128, Duration::from_secs(2)),
    )
    .await
    .expect("bounded command succeeds");
    assert!(ok);
    assert!(output.contains("stdout-value"));
    assert!(output.contains("--- stderr ---"));
    assert!(output.contains("stderr-value"));
}

#[tokio::test]
async fn rejects_stdout_before_the_display_truncation_layer() {
    let error = run_cmd_with_limits(
        None,
        "sh",
        &["-c", "head -c 4096 /dev/zero"],
        limits(128, 128, Duration::from_secs(2)),
    )
    .await
    .expect_err("stdout overflow must fail");
    assert!(error.contains("stdout exceeded the 128-byte limit"));
}

#[tokio::test]
async fn rejects_stderr_before_the_display_truncation_layer() {
    let error = run_cmd_with_limits(
        None,
        "sh",
        &["-c", "head -c 4096 /dev/zero >&2"],
        limits(128, 128, Duration::from_secs(2)),
    )
    .await
    .expect_err("stderr overflow must fail");
    assert!(error.contains("stderr exceeded the 128-byte limit"));
}

#[tokio::test]
async fn timeout_kills_and_reaps_the_child_promptly() {
    let started = Instant::now();
    let error = run_cmd_with_limits(
        None,
        "sleep",
        &["5"],
        limits(128, 128, Duration::from_millis(100)),
    )
    .await
    .expect_err("timeout must fail");
    assert!(error.contains("timed out"));
    assert!(started.elapsed() < Duration::from_secs(2));
}
