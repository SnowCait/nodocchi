use std::fs::File;
use std::path::{Path, PathBuf};

use tracing::Subscriber;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("failed to open log file {path}: {source}")]
    OpenLogFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn init(log_file: Option<&Path>) -> Result<Option<WorkerGuard>, LoggingError> {
    let subscriber = tracing_subscriber::registry()
        .with(env_filter())
        .with(fmt::layer());

    let Some(path) = log_file else {
        subscriber.init();
        return Ok(None);
    };

    let (writer, guard) = tracing_appender::non_blocking(open_log_file(path)?);
    subscriber.with(file_layer(writer)).init();
    Ok(Some(guard))
}

fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

fn open_log_file(path: &Path) -> Result<File, LoggingError> {
    File::options()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| LoggingError::OpenLogFile {
            path: path.to_path_buf(),
            source,
        })
}

fn file_layer<S>(writer: NonBlocking) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fmt::layer().with_ansi(false).with_writer(writer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "riichilab-client-{name}-{}.log",
            std::process::id()
        ))
    }

    #[test]
    fn file_layer_writes_events_without_ansi_escape_sequence() {
        let path = temp_log_path("no-ansi");
        let _ = std::fs::remove_file(&path);

        let (writer, guard) = tracing_appender::non_blocking(open_log_file(&path).unwrap());
        let subscriber = tracing_subscriber::registry().with(file_layer(writer));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(request_id = 1, "action sent");
        });
        drop(guard);

        let contents = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert!(contents.contains("action sent"), "{contents}");
        assert!(contents.contains("request_id=1"), "{contents}");
        assert!(!contents.contains('\u{1b}'), "{contents:?}");
    }

    #[test]
    fn open_log_file_fails_for_missing_directory() {
        let path = temp_log_path("missing-dir").join("nested.log");
        let error = open_log_file(&path).unwrap_err();
        let LoggingError::OpenLogFile {
            path: reported_path,
            ..
        } = error;
        assert_eq!(reported_path, path);
    }
}
