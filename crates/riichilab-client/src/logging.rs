use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tracing::{Metadata, Subscriber};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::filter::{FilterFn, filter_fn};
use tracing_subscriber::layer::Context;
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
pub(crate) const SLOW_REQUEST_TARGET: &str = "riichilab_client::slow_request";

#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("failed to open log file {path}: {source}")]
    OpenLogFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub struct LoggingGuard {
    _file_guard: WorkerGuard,
    _slow_file_guard: WorkerGuard,
    slow_request_count: Arc<AtomicUsize>,
    slow_log_path: PathBuf,
}

impl LoggingGuard {
    /// 正常終了後に呼び、slow request があった起動だけ console へ1回通知する。
    pub fn warn_if_slow_requests_recorded(&self) {
        let count = self.slow_request_count.load(Ordering::Relaxed);
        let _ = write_slow_request_completion_warning(
            &mut io::stderr().lock(),
            count,
            &self.slow_log_path,
        );
    }
}

pub fn init(log_file: Option<&Path>) -> Result<Option<LoggingGuard>, LoggingError> {
    let rust_log = std::env::var("RUST_LOG").ok();
    let policy = resolve_filter_policy(rust_log.as_deref(), log_file.is_some());
    let subscriber = tracing_subscriber::registry().with(
        fmt::layer()
            .with_filter(env_filter(&policy.console))
            .with_filter(regular_log_filter()),
    );

    let Some(path) = log_file else {
        subscriber.init();
        return Ok(None);
    };

    let (writer, file_guard) = tracing_appender::non_blocking(open_log_file(path)?);
    let slow_log_path = slow_log_path(path);
    let (slow_writer, slow_file_guard) =
        tracing_appender::non_blocking(open_log_file(&slow_log_path)?);
    let slow_request_count = Arc::new(AtomicUsize::new(0));
    subscriber
        .with(
            file_layer(writer)
                .with_filter(env_filter(
                    policy.file.as_deref().expect("file filter should exist"),
                ))
                .with_filter(regular_log_filter()),
        )
        .with(file_layer(slow_writer).with_filter(slow_request_filter()))
        .with(
            SlowRequestCounter::new(Arc::clone(&slow_request_count))
                .with_filter(slow_request_filter()),
        )
        .init();
    Ok(Some(LoggingGuard {
        _file_guard: file_guard,
        _slow_file_guard: slow_file_guard,
        slow_request_count,
        slow_log_path,
    }))
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

pub fn slow_log_path(log_file: &Path) -> PathBuf {
    let mut file_name = log_file
        .file_stem()
        .unwrap_or_else(|| log_file.as_os_str())
        .to_os_string();
    file_name.push("-slow.log");
    log_file.with_file_name(file_name)
}

fn slow_request_filter() -> FilterFn<impl Fn(&Metadata<'_>) -> bool + Clone> {
    filter_fn(|metadata| metadata.target() == SLOW_REQUEST_TARGET)
}

fn regular_log_filter() -> FilterFn<impl Fn(&Metadata<'_>) -> bool + Clone> {
    filter_fn(|metadata| metadata.target() != SLOW_REQUEST_TARGET)
}

#[derive(Clone)]
struct SlowRequestCounter {
    count: Arc<AtomicUsize>,
}

impl SlowRequestCounter {
    fn new(count: Arc<AtomicUsize>) -> Self {
        Self { count }
    }
}

impl<S> Layer<S> for SlowRequestCounter
where
    S: Subscriber,
{
    fn on_event(&self, _event: &tracing::Event<'_>, _context: Context<'_, S>) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

fn write_slow_request_completion_warning(
    writer: &mut impl Write,
    count: usize,
    path: &Path,
) -> io::Result<()> {
    if count > 0 {
        writeln!(
            writer,
            "WARN {count} slow request_action responses recorded: {}",
            path.display()
        )?;
    }
    Ok(())
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
pub(crate) fn temp_log_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "riichilab-client-{name}-{}.log",
        std::process::id()
    ))
}

/// `--log-file` の investigation preset をそのまま適用した file layer で `f` の出力を取得する。
#[cfg(test)]
pub(crate) fn capture_investigation_log(name: &str, f: impl FnOnce()) -> String {
    let path = temp_log_path(name);
    let _ = std::fs::remove_file(&path);

    let (writer, guard) = tracing_appender::non_blocking(open_log_file(&path).unwrap());
    let subscriber = tracing_subscriber::registry()
        .with(file_layer(writer).with_filter(env_filter(INVESTIGATION_FILE_FILTER)));
    tracing::subscriber::with_default(subscriber, f);
    drop(guard);

    let contents = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    contents
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn slow_log_path_adds_suffix_before_log_extension() {
        assert_eq!(
            slow_log_path(Path::new("foo.log")),
            PathBuf::from("foo-slow.log")
        );
    }

    #[test]
    fn slow_log_path_keeps_parent_directory() {
        assert_eq!(
            slow_log_path(Path::new("logs/ranked.log")),
            PathBuf::from("logs/ranked-slow.log")
        );
    }

    #[test]
    fn slow_layer_records_only_its_target_with_required_fields() {
        let path = temp_log_path("slow-target-filter");
        let _ = std::fs::remove_file(&path);

        let (writer, guard) = tracing_appender::non_blocking(open_log_file(&path).unwrap());
        let subscriber = tracing_subscriber::registry()
            .with(file_layer(writer).with_filter(slow_request_filter()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "unrelated", "unrelated warn excluded");
            tracing::info!(target: "unrelated", "unrelated info excluded");
            tracing::warn!(
                target: SLOW_REQUEST_TARGET,
                request_id = 42,
                response_type = "dahai",
                context_ms = 10,
                policy_ms = 20,
                serialize_ms = 1,
                send_ms = 2,
                total_ms = 33,
                grace_ms = ?Some(30_u64),
                deadline_ms = ?Some(100_u64),
                "slow request_action response"
            );
        });
        drop(guard);

        let contents = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        for included in [
            "slow request_action response",
            "request_id=42",
            "response_type=\"dahai\"",
            "context_ms=10",
            "policy_ms=20",
            "serialize_ms=1",
            "send_ms=2",
            "total_ms=33",
            "grace_ms=Some(30)",
            "deadline_ms=Some(100)",
        ] {
            assert!(contents.contains(included), "{included}: {contents}");
        }
        assert!(!contents.contains("unrelated warn excluded"), "{contents}");
        assert!(!contents.contains("unrelated info excluded"), "{contents}");
    }

    #[test]
    fn slow_layer_ignores_rust_log_filter_used_by_other_layers() {
        let normal_path = temp_log_path("slow-rust-log-normal");
        let slow_path = temp_log_path("slow-rust-log-dedicated");
        let _ = std::fs::remove_file(&normal_path);
        let _ = std::fs::remove_file(&slow_path);

        let (normal_writer, normal_guard) =
            tracing_appender::non_blocking(open_log_file(&normal_path).unwrap());
        let (slow_writer, slow_guard) =
            tracing_appender::non_blocking(open_log_file(&slow_path).unwrap());
        let subscriber = tracing_subscriber::registry()
            .with(file_layer(normal_writer).with_filter(env_filter("unrelated=error")))
            .with(file_layer(slow_writer).with_filter(slow_request_filter()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(
                target: SLOW_REQUEST_TARGET,
                request_id = 43,
                "slow request_action response"
            );
        });
        drop(normal_guard);
        drop(slow_guard);

        let normal_contents = std::fs::read_to_string(&normal_path).unwrap();
        let slow_contents = std::fs::read_to_string(&slow_path).unwrap();
        let _ = std::fs::remove_file(&normal_path);
        let _ = std::fs::remove_file(&slow_path);

        assert!(!normal_contents.contains("slow request_action response"));
        assert!(
            slow_contents.contains("slow request_action response"),
            "{slow_contents}"
        );
    }

    #[test]
    fn slow_counter_counts_only_slow_target_events() {
        let count = Arc::new(AtomicUsize::new(0));
        let subscriber = tracing_subscriber::registry()
            .with(SlowRequestCounter::new(Arc::clone(&count)).with_filter(slow_request_filter()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(target: "unrelated", "unrelated warn");
            tracing::warn!(target: SLOW_REQUEST_TARGET, "slow request_action response");
        });

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn completion_warning_is_omitted_for_zero() {
        let path = Path::new("logs/ranked-slow.log");
        let mut output = Vec::new();

        write_slow_request_completion_warning(&mut output, 0, path).unwrap();

        assert!(output.is_empty());
    }

    #[test]
    fn completion_warning_bypasses_the_tracing_console_filter() {
        let path = Path::new("logs/ranked-slow.log");
        let mut output = Vec::new();
        let subscriber = tracing_subscriber::registry().with(
            fmt::layer()
                .with_writer(io::sink)
                .with_filter(env_filter("error")),
        );

        tracing::subscriber::with_default(subscriber, || {
            write_slow_request_completion_warning(&mut output, 2, path).unwrap();
        });

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "WARN 2 slow request_action responses recorded: logs/ranked-slow.log\n"
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
