use clap::Parser;
#[tokio::main]
async fn main() {
    if let Err(error) = hermes_grocy_mcp::cli::run(hermes_grocy_mcp::cli::Cli::parse()).await {
        eprintln!("{error}");
        std::process::exit(1)
    }
}
