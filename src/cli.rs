use crate::{
    credentials::{
        CredentialStore, DesktopStore, preferences::PreferenceStore, protection, providers,
    },
    error::{AppError, Result},
    model::{AccessMode, ProfileSelection, TransportPolicy},
    setup::{SetupOptions, SetupUi},
};
use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input, Password, Select};
use std::{path::PathBuf, sync::Arc};
#[derive(Parser)]
#[command(
    version,
    about = "Protected local household Grocy MCP. Start with setup."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
#[derive(clap::Args)]
pub struct ProfileArgs {
    #[arg(long, default_value = "default")]
    pub profile: String,
    #[arg(long, help = "Use only this previously selected wallet service")]
    pub provider: Option<String>,
}
#[derive(Subcommand)]
pub enum Command {
    #[command(about = "Guided wallet enrollment and Hermes connection; key input is hidden")]
    Setup {
        #[arg(long, default_value = "default")]
        profile: String,
        #[arg(long)]
        ca_bundle: Option<PathBuf>,
        #[arg(long)]
        read_only: bool,
        #[arg(long)]
        no_hermes: bool,
        #[arg(
            long,
            help = "Explicit development enrollment: HTTP allowed only for a literal loopback destination"
        )]
        allow_loopback_http: bool,
    },
    #[command(about = "Run the stdio MCP using an enrolled wallet profile")]
    Serve(ProfileArgs),
    #[command(about = "Check the wallet and API without household mutations")]
    Doctor(ProfileArgs),
    #[command(about = "Discover wallet providers without reading unrelated secrets")]
    Wallets,
    #[command(about = "Print a credential-free Hermes MCP entry")]
    PrintConfig(ProfileArgs),
    #[command(about = "Add the entry using Hermes's preserving writer and check discovery")]
    ConnectHermes(ProfileArgs),
    #[command(
        about = "Remove only this application's local wallet profile; server-key revocation is separate"
    )]
    Forget(ProfileArgs),
}
fn io_error() -> AppError {
    AppError::new(
        "INPUT_CANCELLED",
        "Input was cancelled or unavailable. Run setup in an interactive terminal.",
    )
}
pub struct ConsoleUi;
impl SetupUi for ConsoleUi {
    fn message(&mut self, s: &str) {
        println!("{s}")
    }
    fn input(&mut self, prompt: &str, default: Option<&str>) -> Result<String> {
        let mut i = Input::<String>::new().with_prompt(prompt);
        if let Some(d) = default {
            i = i.default(d.into())
        }
        i.interact_text().map_err(|_| io_error())
    }
    fn secret(&mut self, prompt: &str, allow_empty: bool) -> Result<secrecy::SecretString> {
        Password::new()
            .with_prompt(prompt)
            .allow_empty_password(allow_empty)
            .interact()
            .map(secrecy::SecretString::from)
            .map_err(|_| io_error())
    }
    fn choose(&mut self, prompt: &str, options: &[String], default: usize) -> Result<usize> {
        Select::new()
            .with_prompt(prompt)
            .items(options)
            .default(default)
            .interact()
            .map_err(|_| io_error())
    }
    fn confirm(&mut self, prompt: &str, default: bool) -> Result<bool> {
        Confirm::new()
            .with_prompt(prompt)
            .default(default)
            .interact()
            .map_err(|_| io_error())
    }
    fn providers(&mut self) -> Result<Vec<providers::ProviderCandidate>> {
        providers::discover_providers()
    }
    fn protection(&mut self, s: &ProfileSelection) -> protection::ProtectionEvidence {
        protection::inspect_protection(&s.provider)
    }
}
fn selection(args: &ProfileArgs, prefs: &PreferenceStore) -> Result<ProfileSelection> {
    let s = prefs.load(&args.profile)?;
    if args
        .provider
        .as_ref()
        .is_some_and(|p| p != &s.provider.bus_name)
    {
        return Err(AppError::new(
            "WALLET_PROVIDER_CHANGED",
            "The requested provider differs from enrollment. Run setup to select another wallet.",
        ));
    }
    Ok(s)
}
pub async fn run(cli: Cli) -> Result<()> {
    let prefs = PreferenceStore::default_location()?;
    let store: Arc<dyn CredentialStore> = Arc::new(DesktopStore);
    match cli.command {
        Command::Setup {
            profile,
            ca_bundle,
            read_only,
            no_hermes,
            allow_loopback_http,
        } => {
            crate::setup::run(
                SetupOptions {
                    profile,
                    preferences: prefs,
                    ca_path: ca_bundle,
                    access: if read_only {
                        Some(AccessMode::ReadOnly)
                    } else {
                        None
                    },
                    transport: if allow_loopback_http {
                        TransportPolicy::LoopbackDevelopment
                    } else {
                        TransportPolicy::HttpsOnly
                    },
                    connect_hermes: !no_hermes,
                },
                &mut ConsoleUi,
                store,
            )
            .await?;
        }
        Command::Serve(a) => crate::mcp::serve_stdio(store, selection(&a, &prefs)?).await?,
        Command::Doctor(a) => {
            let report = crate::setup::doctor(selection(&a, &prefs)?, store).await?;
            println!(
                "Wallet profile loaded with encrypted communication. API reachable; household access checked. Grocy {}. No household changes made.",
                report.version.as_deref().unwrap_or("version unknown")
            );
        }
        Command::Wallets => {
            for p in providers::discover_providers()? {
                println!(
                    "{}{}",
                    p.label,
                    if p.recommended { " (recommended)" } else { "" }
                );
            }
        }
        Command::PrintConfig(a) => println!(
            "{}",
            crate::setup::hermes::render_config(
                &std::env::current_exe().map_err(|_| io_error())?,
                &selection(&a, &prefs)?
            )?
        ),
        Command::ConnectHermes(a) => {
            crate::setup::hermes::connect(
                &std::env::current_exe().map_err(|_| io_error())?,
                &selection(&a, &prefs)?,
            )
            .await?;
            println!(
                "Hermes entry and discovery checked. Run /reload-mcp in Hermes or start a new session."
            );
        }
        Command::Forget(a) => {
            let s = selection(&a, &prefs)?;
            if Confirm::new()
                .with_prompt(
                    "Remove this local Grocy profile? This does not revoke its server API key.",
                )
                .default(false)
                .interact()
                .map_err(|_| io_error())?
            {
                store.forget(s).await?;
                prefs.remove(&a.profile)?;
                println!(
                    "Local profile removed. Revoke its API key in Grocy > Manage API keys; remove Hermes's grocy entry if no longer needed."
                );
            }
        }
    }
    Ok(())
}
