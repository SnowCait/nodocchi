use bot_core::NormalAgent;
use riichilab_client::validation_policy::AgentKind;
use riichilab_client::{ClientConfig, install_default_crypto_provider, run_validation_client};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_default_crypto_provider();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = ClientConfig::from_env()?;
    let agent_kind = AgentKind::from_env()?;
    let policy = agent_kind.response_policy();
    let mut agent = NormalAgent;

    info!(agent_kind = %agent_kind, "selected agent");

    run_validation_client(config, &mut agent, policy).await?;
    Ok(())
}
