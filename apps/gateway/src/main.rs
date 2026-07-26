use anyhow::Result;
use benshu_gateway::LaunchOptions;
use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Parser)]
#[command(name = "benshu-gw")]
#[command(about = "BenShu AI Gateway - Lightweight tool execution engine", long_about = None)]
struct Cli {
    /// Custom data directory for models, logs, and runtimes
    #[arg(long, env = "BENSHU_DATA_DIR")]
    data_dir: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all available skills
    List,
    /// Run diagnostic checks
    Doctor,
    /// Run onboarding wizard
    Onboard,
    /// Run a specific skill
    Run {
        /// Name of the skill to run
        name: String,
        /// JSON arguments for the skill
        #[arg(default_value = "{}")]
        args: String,
    },
    /// Start the gateway server (MCP)
    Serve,
    /// Start the HTTP API server
    Web {
        /// Port to listen on
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
        /// Explicitly choose the LLM provider
        #[arg(long)]
        provider: Option<String>,
        /// Explicitly choose the model name
        #[arg(long)]
        model: Option<String>,
    },
}

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()?;

    runtime.block_on(async_main())
}

async fn async_main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    // --- Unified Path Resolution (Storage Root) ---
    let (base_dir, _is_portable) = if let Some(dir) = &cli.data_dir {
        (dir.clone(), false)
    } else {
        let exe_path = std::env::current_exe().unwrap_or_default();
        let exe_dir = exe_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_path_buf();

        // Detect if we are in Windows "restricted" zones
        let is_system_restricted = cfg!(windows)
            && (exe_dir
                .to_string_lossy()
                .to_lowercase()
                .contains("program files")
                || exe_dir
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("windows\\system32"));

        if is_system_restricted {
            (
                dirs::data_local_dir()
                    .map(|d| d.join("benshu").join("data"))
                    .unwrap_or_else(|| exe_dir.join("data")),
                false,
            )
        } else {
            (exe_dir.join("data"), true)
        }
    };

    std::env::set_var("BENSHU_DATA_DIR", &base_dir);

    match cli.command {
        Commands::List => {
            println!(
                "Scanning skills in {}...",
                base_dir.join("skills").display()
            );
        }
        Commands::Doctor => {
            benshu_gateway::doctor::run_doctor().await?;
        }
        Commands::Onboard => {
            benshu_gateway::onboard::run_onboard().await?;
        }
        Commands::Run { name, args } => {
            info!("Running skill: {} with args: {}", name, args);
        }
        Commands::Serve => {
            info!("Launching BenShu engine at {}...", base_dir.display());
            benshu_gateway::launch_engine(base_dir, LaunchOptions::standalone(None), None).await?;
        }
        Commands::Web { port, .. } => {
            info!("Launching BenShu engine at {}...", base_dir.display());
            benshu_gateway::launch_engine(base_dir, LaunchOptions::standalone(Some(port)), None)
                .await?;
        }
    }

    Ok(())
}
