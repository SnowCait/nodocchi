use riichilab_client::logging;

#[test]
fn init_writes_events_to_log_file_without_ansi() {
    let path =
        std::env::temp_dir().join(format!("riichilab-client-init-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let guard = logging::init(Some(&path))
        .expect("log file should be opened")
        .expect("worker guard should be returned");
    tracing::info!(request_id = 7, "action sent");
    drop(guard);

    let contents = std::fs::read_to_string(&path).expect("log file should exist");
    let _ = std::fs::remove_file(&path);

    assert!(contents.contains("action sent"), "{contents}");
    assert!(contents.contains("request_id=7"), "{contents}");
    assert!(!contents.contains('\u{1b}'), "{contents:?}");
}
