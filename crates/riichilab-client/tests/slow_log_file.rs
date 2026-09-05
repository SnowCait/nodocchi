use riichilab_client::logging;

const SLOW_REQUEST_TARGET: &str = "riichilab_client::slow_request";

#[test]
fn init_creates_the_slow_log_on_the_first_slow_request() {
    let path = std::env::temp_dir().join(format!(
        "riichilab-client-slow-init-{}.log",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let slow_path = logging::slow_log_path(&path);
    let _ = std::fs::remove_file(&slow_path);

    let guard = logging::init(Some(&path))
        .expect("log file should be opened")
        .expect("worker guard should be returned");
    tracing::info!(request_id = 7, "action sent");
    assert!(
        !slow_path.exists(),
        "slow log file should not be created before the first slow request"
    );

    tracing::warn!(
        target: SLOW_REQUEST_TARGET,
        request_id = 8,
        total_ms = 900,
        "slow request_action response"
    );
    tracing::warn!(
        target: SLOW_REQUEST_TARGET,
        request_id = 9,
        total_ms = 950,
        "slow request_action response"
    );
    drop(guard);

    let contents = std::fs::read_to_string(&path).expect("log file should exist");
    let slow_contents = std::fs::read_to_string(&slow_path).expect("slow log file should exist");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&slow_path);

    assert!(
        !contents.contains("slow request_action response"),
        "{contents}"
    );
    assert!(!slow_contents.contains("action sent"), "{slow_contents}");
    assert!(slow_contents.contains("request_id=8"), "{slow_contents}");
    assert!(slow_contents.contains("request_id=9"), "{slow_contents}");
    assert_eq!(
        slow_contents
            .lines()
            .filter(|line| line.contains("slow request_action response"))
            .count(),
        2,
        "{slow_contents}"
    );
    assert!(!slow_contents.contains('\u{1b}'), "{slow_contents:?}");
}
