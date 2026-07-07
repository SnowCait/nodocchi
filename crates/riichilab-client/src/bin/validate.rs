use bot_core::AlwaysLegalAgent;
use riichilab_client::{ClientConfig, install_default_crypto_provider, run_validation_client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_default_crypto_provider();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = ClientConfig::from_env()?;
    let mut agent = AlwaysLegalAgent;

    run_validation_client(config, &mut agent).await?;
    Ok(())
}
