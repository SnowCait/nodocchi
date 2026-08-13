use bot_core::NormalAgent;
use riichilab_client::validation_policy::AgentKind;
use riichilab_client::{
    CliArgs, ClientConfig, ClientExitCondition, ConnectionMode, USAGE, capture,
    install_default_crypto_provider, logging, run_riichilab_client,
};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = match CliArgs::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    install_default_crypto_provider();

    let _log_guard = match logging::init(args.log_file.as_deref()) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    let (capture, _capture_guard) = match capture::init(args.capture_file.as_deref()) {
        Ok(capture) => capture.unzip(),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    let config = ClientConfig::from_env_with_endpoint(args.mode.endpoint().to_string())?;
    let agent_kind = match args.agent {
        Some(kind) => kind,
        None => AgentKind::from_env()?,
    };
    let policy = agent_kind.response_policy();
    let mut fallback_agent = NormalAgent;

    let exit_condition = match args.mode {
        ConnectionMode::Validate => ClientExitCondition::ValidationResult,
        ConnectionMode::Ranked => ClientExitCondition::EndGame,
    };

    info!(mode = %args.mode, agent_kind = %agent_kind, "selected mode and agent");
    if let Some(path) = args.capture_file.as_deref() {
        info!(capture_file = %path.display(), "capturing request_action to JSONL");
    }

    run_riichilab_client(config, &mut fallback_agent, policy, exit_condition, capture).await?;
    Ok(())
}
