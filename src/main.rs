mod config;
mod core;
mod tui;

use anyhow::{Context, Result};
use clap::Parser;
use core::{docker::DockerEngine, Engine};
use std::{io::IsTerminal, sync::Arc};

#[tokio::main]
async fn main() -> Result<()> {
    let args = config::Args::parse();
    let endpoint = config::resolve(&args)?;
    let engine: Arc<dyn Engine> = Arc::new(DockerEngine::connect(endpoint).await?);
    if args.check {
        let snapshot = engine.snapshot().await?;
        println!(
            "{}\n{}\n{} containers | {} images | {} volumes | {} networks",
            engine.endpoint(),
            snapshot.engine,
            snapshot.containers.len(),
            snapshot.images.len(),
            snapshot.volumes.len(),
            snapshot.networks.len()
        );
        return Ok(());
    }
    anyhow::ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "Use an interactive terminal, or run docker-goo --check"
    );
    anyhow::ensure!(
        std::env::var("TERM").as_deref() != Ok("dumb"),
        "This terminal has no cursor support; use docker-goo --check"
    );
    tui::run(engine, args.shell, args.console, args.cwd)
        .await
        .context("Docker-Goo session ended")
}
