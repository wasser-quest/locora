//! `locora`: command line for memory palaces and plugin development.

mod plugin;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "locora",
    version,
    about = "Gedächtnispaläste und Plugins für Locora"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Plugins erstellen, entwickeln und testen
    #[command(subcommand)]
    Plugin(plugin::PluginCommand),
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Plugin(cmd) => plugin::run(cmd),
    }
}
