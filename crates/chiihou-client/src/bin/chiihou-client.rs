use bot_core::{NormalAgent, ShantenAgent, TsumogiriAgent};
use chiihou_client::{
    ChiihouAgentKind, ChiihouCliArgs, ChiihouRuntimeOptions, USAGE, build_cli_nostr_config,
    load_chiihou_nsec, run_chiihou_client_auto_enter_with_options,
};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = match ChiihouCliArgs::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let nsec = load_chiihou_nsec()?;
    let config = build_cli_nostr_config(&nsec, &args)?;
    let options = ChiihouRuntimeOptions {
        auto_next: args.auto_next,
        response_delay: args.response_delay(),
    };

    let server_source = if args.server_npub.is_some() {
        "cli"
    } else {
        "default"
    };

    info!(
        channel = %args.channel,
        agent = %args.agent,
        auto_next = args.auto_next,
        response_delay_ms = args.response_delay_ms,
        relay_count = config.relay_urls().len(),
        server_source,
        "starting chiihou client"
    );

    match args.agent {
        ChiihouAgentKind::Normal => {
            let mut agent = NormalAgent;
            run_chiihou_client_auto_enter_with_options(&config, options, &mut agent).await?;
        }
        ChiihouAgentKind::Tsumogiri => {
            let mut agent = TsumogiriAgent;
            run_chiihou_client_auto_enter_with_options(&config, options, &mut agent).await?;
        }
        ChiihouAgentKind::Shanten => {
            let mut agent = ShantenAgent;
            run_chiihou_client_auto_enter_with_options(&config, options, &mut agent).await?;
        }
    }

    Ok(())
}
