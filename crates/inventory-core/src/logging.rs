//! Rotating file logging with secret redaction. Secrets must never reach disk.

use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"ghp_[A-Za-z0-9]{20,}").unwrap(),
        Regex::new(r"github_pat_[A-Za-z0-9_]{20,}").unwrap(),
        Regex::new(r"(?i)bearer\s+[^\s]+").unwrap(),
        Regex::new(r"(?i)(client_secret|api_key|token|password)\s*[=:]\s*[^\s]+").unwrap(),
    ]
});

/// Replace anything that looks like a credential with `[REDACTED]`.
pub fn redact(input: &str) -> String {
    let mut out = input.to_string();
    for re in SECRET_PATTERNS.iter() {
        out = re.replace_all(&out, "[REDACTED]").into_owned();
    }
    out
}

struct RedactingWriter<W: Write>(W);

// SAFETY-RELEVANT INVARIANT: redaction operates per `write` call, with no
// internal buffering. A secret split across two `write` calls would evade the
// patterns. This is sound in the current pipeline because tracing's fmt layer
// emits each formatted event as exactly one `write_all`, and the non-blocking
// channel forwards it unsplit. Do not route partial/streamed writes through
// this writer without adding line buffering first.
impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.0.write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Build a non-blocking, daily-rolling, redacting writer for `log_dir`.
pub fn file_writer(log_dir: &Path) -> std::io::Result<(NonBlocking, WorkerGuard)> {
    std::fs::create_dir_all(log_dir)?;
    let appender = tracing_appender::rolling::daily(log_dir, "app.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(RedactingWriter(appender));
    Ok((non_blocking, guard))
}

/// Install the global subscriber writing to `log_dir`. Keep the guard alive
/// for the life of the process or buffered lines are lost.
pub fn init(log_dir: &Path) -> std::io::Result<WorkerGuard> {
    let (writer, guard) = file_writer(log_dir)?;
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        // Intentional env read: RUST_LOG is a standard operational override for
        // log verbosity. Unlike `paths`, this module does not promise env-free
        // purity; defaults to "info" when RUST_LOG is unset.
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .finish();
    // Ignore the error if a subscriber is already set (e.g. tests).
    let _ = tracing::subscriber::set_global_default(subscriber);
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_github_classic_tokens() {
        let msg = "publish failed for token ghp_abcdefghijklmnopqrstuvwxyz012345 on repo";
        let out = redact(msg);
        assert!(!out.contains("ghp_abcdefghijklmnopqrstuvwxyz012345"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_fine_grained_tokens_and_bearer_headers() {
        let out = redact("Authorization: Bearer github_pat_11ABCDEFG0123456789_abcdefghij");
        assert!(!out.contains("github_pat_"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_client_secrets() {
        let out = redact("request client_secret=SuP3rS3cretValue123 sent");
        assert!(!out.contains("SuP3rS3cretValue123"));
    }

    #[test]
    fn leaves_normal_text_alone() {
        assert_eq!(redact("received 30 x 10k resistor"), "received 30 x 10k resistor");
    }

    #[test]
    fn log_file_is_written_with_redaction_applied() {
        let dir = tempfile::tempdir().unwrap();
        {
            let (writer, guard) = file_writer(dir.path()).unwrap();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(writer)
                .with_ansi(false)
                .finish();
            tracing::subscriber::with_default(subscriber, || {
                tracing::info!("startup with ghp_abcdefghijklmnopqrstuvwxyz012345");
            });
            drop(guard); // flush
        }
        let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert!(!entries.is_empty(), "expected a log file");
        let content = std::fs::read_to_string(entries[0].path()).unwrap();
        assert!(content.contains("[REDACTED]"));
        assert!(!content.contains("ghp_abcdefghijklmnopqrstuvwxyz012345"));
    }
}
