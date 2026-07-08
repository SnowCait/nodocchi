use bot_core::NormalAgent;
use riichilab_client::validation_policy::AgentKind;
use riichilab_client::{
    CliArgs, ClientConfig, USAGE, install_default_crypto_provider, run_validation_client,
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

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = ClientConfig::from_env_with_endpoint(args.mode.endpoint().to_string())?;
    let agent_kind = match args.agent {
        Some(kind) => kind,
        None => AgentKind::from_env()?,
    };
    let policy = agent_kind.response_policy();
    let mut fallback_agent = NormalAgent;

    info!(mode = %args.mode, agent_kind = %agent_kind, "selected mode and agent");

    run_validation_client(config, &mut fallback_agent, policy).await?;
    Ok(())
}
