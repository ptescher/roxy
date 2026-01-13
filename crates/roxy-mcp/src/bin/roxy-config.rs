//! Roxy Configuration CLI
//!
//! Manage Roxy configuration stored in OS-appropriate location

use anyhow::Result;
use clap::{Parser, Subcommand};
use roxy_core::RoxyConfig;

#[derive(Parser)]
#[command(name = "roxy-config")]
#[command(about = "Manage Roxy configuration", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show current configuration
    Show,

    /// Show configuration file path
    Path,

    /// Set Datadog API credentials
    SetDatadog {
        /// Datadog API key
        #[arg(long)]
        api_key: String,

        /// Datadog application key
        #[arg(long)]
        app_key: String,

        /// Datadog site (default: datadoghq.com)
        #[arg(long, default_value = "datadoghq.com")]
        site: String,
    },

    /// Initialize configuration from environment variables
    InitFromEnv,

    /// Reset configuration to defaults
    Reset,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Show => {
            let config = RoxyConfig::load_with_env()?;
            println!(
                "Configuration file: {}",
                RoxyConfig::config_path()?.display()
            );
            println!("\n{}", toml::to_string_pretty(&config)?);

            // Show which values come from environment
            if std::env::var("DD_API_KEY").is_ok() {
                println!("\n⚠️  DD_API_KEY is overridden by environment variable");
            }
            if std::env::var("DD_APP_KEY").is_ok() {
                println!("⚠️  DD_APP_KEY is overridden by environment variable");
            }
            if std::env::var("DD_SITE").is_ok() {
                println!("⚠️  DD_SITE is overridden by environment variable");
            }
        }

        Commands::Path => {
            println!("{}", RoxyConfig::config_path()?.display());
        }

        Commands::SetDatadog {
            api_key,
            app_key,
            site,
        } => {
            let mut config = RoxyConfig::load()?;
            config.datadog.api_key = Some(api_key);
            config.datadog.app_key = Some(app_key);
            config.datadog.site = site;
            config.save()?;

            println!(
                "✓ Datadog credentials saved to {}",
                RoxyConfig::config_path()?.display()
            );
            println!("\nConfiguration updated. Restart roxy-mcp for changes to take effect.");
        }

        Commands::InitFromEnv => {
            let mut config = RoxyConfig::load()?;
            let mut updated = false;

            if let Ok(api_key) = std::env::var("DD_API_KEY") {
                config.datadog.api_key = Some(api_key);
                updated = true;
                println!("✓ Set DD_API_KEY from environment");
            }

            if let Ok(app_key) = std::env::var("DD_APP_KEY") {
                config.datadog.app_key = Some(app_key);
                updated = true;
                println!("✓ Set DD_APP_KEY from environment");
            }

            if let Ok(site) = std::env::var("DD_SITE") {
                config.datadog.site = site;
                updated = true;
                println!("✓ Set DD_SITE from environment");
            }

            if updated {
                config.save()?;
                println!(
                    "\n✓ Configuration saved to {}",
                    RoxyConfig::config_path()?.display()
                );
            } else {
                println!("No environment variables found (DD_API_KEY, DD_APP_KEY, DD_SITE)");
            }
        }

        Commands::Reset => {
            let config = RoxyConfig::default();
            config.save()?;
            println!(
                "✓ Configuration reset to defaults at {}",
                RoxyConfig::config_path()?.display()
            );
        }
    }

    Ok(())
}
