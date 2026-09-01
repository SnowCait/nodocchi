use std::fs::File;
use std::path::{Path, PathBuf};

use tracing::Subscriber;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

const DEFAULT_CONSOLE_FILTER: &str = "info";
const INVESTIGATION_FILE_FILTER: &str = "info,\
    bot_core::agent_decision=debug,\
    bot_core::push_pull=debug,\
    bot_core::discard_selection=trace,\
    bot_core::defense=trace";

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
    let rust_log = std::env::var("RUST_LOG").ok();
    let policy = resolve_filter_policy(rust_log.as_deref(), log_file.is_some());
    let subscriber =
        tracing_subscriber::registry().with(fmt::layer().with_filter(env_filter(&policy.console)));

    let Some(path) = log_file else {
        subscriber.init();
        return Ok(None);
    };

    let (writer, guard) = tracing_appender::non_blocking(open_log_file(path)?);
    subscriber
        .with(file_layer(writer).with_filter(env_filter(
            policy.file.as_deref().expect("file filter should exist"),
        )))
        .init();
    Ok(Some(guard))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FilterPolicy {
    console: String,
    file: Option<String>,
}

// 環境変数の読み取りとは分離し、明示 RUST_LOG と file の有無だけから各 layer の policy を決める。
// RUST_LOG があれば preset を足さず、console / file の両方で同じ明示値を source of truth にする。
fn resolve_filter_policy(rust_log: Option<&str>, has_log_file: bool) -> FilterPolicy {
    let console = rust_log.unwrap_or(DEFAULT_CONSOLE_FILTER).to_string();
    let file = has_log_file.then(|| rust_log.unwrap_or(INVESTIGATION_FILE_FILTER).to_string());
    FilterPolicy { console, file }
}

fn env_filter(directives: &str) -> EnvFilter {
    EnvFilter::try_new(directives).unwrap_or_else(|_| EnvFilter::new(DEFAULT_CONSOLE_FILTER))
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
    fn no_file_without_rust_log_uses_info_console_only() {
        assert_eq!(
            resolve_filter_policy(None, false),
            FilterPolicy {
                console: DEFAULT_CONSOLE_FILTER.to_string(),
                file: None,
            }
        );
    }

    #[test]
    fn file_without_rust_log_uses_the_investigation_preset_only_for_file() {
        assert_eq!(
            resolve_filter_policy(None, true),
            FilterPolicy {
                console: DEFAULT_CONSOLE_FILTER.to_string(),
                file: Some(INVESTIGATION_FILE_FILTER.to_string()),
            }
        );
    }

    #[test]
    fn rust_log_without_file_overrides_the_console_filter() {
        let directives = "bot_core::push_pull=trace";
        assert_eq!(
            resolve_filter_policy(Some(directives), false),
            FilterPolicy {
                console: directives.to_string(),
                file: None,
            }
        );
    }

    #[test]
    fn rust_log_with_file_overrides_both_filters_without_adding_the_preset() {
        let directives = "bot_core::push_pull=trace";
        assert_eq!(
            resolve_filter_policy(Some(directives), true),
            FilterPolicy {
                console: directives.to_string(),
                file: Some(directives.to_string()),
            }
        );
    }

    #[test]
    fn per_layer_filters_keep_console_quiet_and_record_investigation_targets() {
        let console_path = temp_log_path("investigation-console-filter");
        let file_path = temp_log_path("investigation-file-filter");
        let _ = std::fs::remove_file(&console_path);
        let _ = std::fs::remove_file(&file_path);

        let (console_writer, console_guard) =
            tracing_appender::non_blocking(open_log_file(&console_path).unwrap());
        let (file_writer, file_guard) =
            tracing_appender::non_blocking(open_log_file(&file_path).unwrap());
        let subscriber = tracing_subscriber::registry()
            .with(file_layer(console_writer).with_filter(env_filter(DEFAULT_CONSOLE_FILTER)))
            .with(file_layer(file_writer).with_filter(env_filter(INVESTIGATION_FILE_FILTER)));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "unrelated", "unrelated info included");
            tracing::debug!(target: "bot_core::agent_decision", "agent debug included");
            tracing::debug!(target: "bot_core::push_pull", "push pull debug included");
            tracing::trace!(target: "bot_core::discard_selection", "discard trace included");
            tracing::trace!(target: "bot_core::defense", "defense trace included");
            tracing::debug!(target: "unrelated", "unrelated debug excluded");
            tracing::trace!(target: "unrelated", "unrelated trace excluded");
        });
        drop(console_guard);
        drop(file_guard);

        let console_contents = std::fs::read_to_string(&console_path).unwrap();
        let file_contents = std::fs::read_to_string(&file_path).unwrap();
        let _ = std::fs::remove_file(&console_path);
        let _ = std::fs::remove_file(&file_path);

        assert!(
            console_contents.contains("unrelated info included"),
            "{console_contents}"
        );
        for excluded in [
            "agent debug included",
            "push pull debug included",
            "discard trace included",
            "defense trace included",
            "unrelated debug excluded",
            "unrelated trace excluded",
        ] {
            assert!(
                !console_contents.contains(excluded),
                "{excluded}: {console_contents}"
            );
        }

        for included in [
            "unrelated info included",
            "agent debug included",
            "push pull debug included",
            "discard trace included",
            "defense trace included",
        ] {
            assert!(
                file_contents.contains(included),
                "{included}: {file_contents}"
            );
        }
        for excluded in ["unrelated debug excluded", "unrelated trace excluded"] {
            assert!(
                !file_contents.contains(excluded),
                "{excluded}: {file_contents}"
            );
        }
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
