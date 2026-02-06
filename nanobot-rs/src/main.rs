//! nanobot - A lightweight personal AI assistant framework (Rust port).

mod agent;
mod bus;
mod channels;
mod config;
mod cron;
mod heartbeat;
mod providers;
mod session;
mod utils;

use std::io::{self, Write as _};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::sync::mpsc;
use tracing::info;

use crate::bus::events::{InboundMessage, OutboundMessage};
use crate::config::loader::{get_config_path, get_data_dir, load_config, save_config};
use crate::config::schema::Config;
use crate::agent::agent_loop::AgentLoop;
use crate::channels::manager::ChannelManager;
use crate::cron::service::CronService;
use crate::cron::types::CronSchedule;
use crate::providers::base::LLMProvider;
use crate::providers::openai_compat::OpenAICompatProvider;
use crate::utils::helpers::get_workspace_path;

const VERSION: &str = "0.1.0";
const LOGO: &str = "\u{1F408}"; // cat emoji

#[derive(Parser)]
#[command(name = "nanobot", about = "nanobot - Personal AI Assistant", version = VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize nanobot configuration and workspace.
    Onboard,
    /// Interact with the agent directly.
    Agent {
        /// Message to send to the agent.
        #[arg(short, long)]
        message: Option<String>,
        /// Session ID.
        #[arg(short, long, default_value = "cli:default")]
        session: String,
    },
    /// Start the nanobot gateway (channels + agent loop).
    Gateway {
        /// Gateway port.
        #[arg(short, long, default_value_t = 18790)]
        port: u16,
        /// Verbose logging.
        #[arg(short, long)]
        verbose: bool,
    },
    /// Show nanobot status.
    Status,
    /// Manage channels.
    Channels {
        #[command(subcommand)]
        action: ChannelsAction,
    },
    /// Manage scheduled tasks.
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
}

#[derive(Subcommand)]
enum ChannelsAction {
    /// Show channel status.
    Status,
}

#[derive(Subcommand)]
enum CronAction {
    /// List scheduled jobs.
    List {
        /// Include disabled jobs.
        #[arg(short, long)]
        all: bool,
    },
    /// Add a scheduled job.
    Add {
        /// Job name.
        #[arg(short, long)]
        name: String,
        /// Message for agent.
        #[arg(short, long)]
        message: String,
        /// Run every N seconds.
        #[arg(short, long)]
        every: Option<u64>,
        /// Cron expression.
        #[arg(short, long)]
        cron: Option<String>,
        /// Deliver response to channel.
        #[arg(short, long)]
        deliver: bool,
        /// Recipient for delivery.
        #[arg(long)]
        to: Option<String>,
        /// Channel for delivery.
        #[arg(long)]
        channel: Option<String>,
    },
    /// Remove a scheduled job.
    Remove {
        /// Job ID to remove.
        job_id: String,
    },
    /// Enable or disable a job.
    Enable {
        /// Job ID.
        job_id: String,
        /// Disable instead of enable.
        #[arg(long)]
        disable: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    match cli.command {
        Commands::Onboard => cmd_onboard(),
        Commands::Agent { message, session } => cmd_agent(message, session),
        Commands::Gateway { port, verbose } => cmd_gateway(port, verbose),
        Commands::Status => cmd_status(),
        Commands::Channels { action } => match action {
            ChannelsAction::Status => cmd_channels_status(),
        },
        Commands::Cron { action } => match action {
            CronAction::List { all } => cmd_cron_list(all),
            CronAction::Add {
                name, message, every, cron, deliver, to, channel,
            } => cmd_cron_add(name, message, every, cron, deliver, to, channel),
            CronAction::Remove { job_id } => cmd_cron_remove(job_id),
            CronAction::Enable { job_id, disable } => cmd_cron_enable(job_id, disable),
        },
    }
}

// ============================================================================
// Onboard
// ============================================================================

fn cmd_onboard() {
    let config_path = get_config_path();

    if config_path.exists() {
        println!("Config already exists at {}", config_path.display());
        print!("Overwrite? [y/N] ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        if !input.trim().eq_ignore_ascii_case("y") {
            return;
        }
    }

    let config = Config::default();
    save_config(&config, None);
    println!("  Created config at {}", config_path.display());

    let workspace = get_workspace_path(None);
    println!("  Created workspace at {}", workspace.display());

    create_workspace_templates(&workspace);

    println!("\n{} nanobot is ready!", LOGO);
    println!("\nNext steps:");
    println!("  1. Add your API key to ~/.nanobot/config.json");
    println!("     Get one at: https://openrouter.ai/keys");
    println!("  2. Chat: nanobot agent -m \"Hello!\"");
}

fn create_workspace_templates(workspace: &std::path::Path) {
    let templates: Vec<(&str, &str)> = vec![
        ("AGENTS.md", "# Agent Instructions\n\nYou are a helpful AI assistant. Be concise, accurate, and friendly.\n\n## Guidelines\n\n- Always explain what you're doing before taking actions\n- Ask for clarification when the request is ambiguous\n- Use tools to help accomplish tasks\n- Remember important information in your memory files\n"),
        ("SOUL.md", "# Soul\n\nI am nanobot, a lightweight AI assistant.\n\n## Personality\n\n- Helpful and friendly\n- Concise and to the point\n- Curious and eager to learn\n\n## Values\n\n- Accuracy over speed\n- User privacy and safety\n- Transparency in actions\n"),
        ("USER.md", "# User\n\nInformation about the user goes here.\n\n## Preferences\n\n- Communication style: (casual/formal)\n- Timezone: (your timezone)\n- Language: (your preferred language)\n"),
    ];

    for (filename, content) in &templates {
        let file_path = workspace.join(filename);
        if !file_path.exists() {
            std::fs::write(&file_path, content).ok();
            println!("  Created {}", filename);
        }
    }

    let memory_dir = workspace.join("memory");
    std::fs::create_dir_all(&memory_dir).ok();
    let memory_file = memory_dir.join("MEMORY.md");
    if !memory_file.exists() {
        std::fs::write(
            &memory_file,
            "# Long-term Memory\n\nThis file stores important information that should persist across sessions.\n",
        )
        .ok();
        println!("  Created memory/MEMORY.md");
    }
}

// ============================================================================
// Agent
// ============================================================================

fn cmd_agent(message: Option<String>, session_id: String) {
    let config = load_config(None);
    let api_key = config.get_api_key();
    let model = config.agents.defaults.model.clone();

    if api_key.is_none() && !model.starts_with("bedrock/") {
        eprintln!("Error: No API key configured.");
        eprintln!("Set one in ~/.nanobot/config.json under providers.openrouter.apiKey");
        std::process::exit(1);
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    runtime.block_on(async {
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel::<InboundMessage>();
        let (outbound_tx, _outbound_rx) = mpsc::unbounded_channel::<OutboundMessage>();

        let provider = create_provider(&config);
        let brave_key = if config.tools.web.search.api_key.is_empty() {
            None
        } else {
            Some(config.tools.web.search.api_key.clone())
        };

        let cron_store_path = get_data_dir().join("cron").join("jobs.json");
        let cron_service = Arc::new(CronService::new(cron_store_path));

        let mut agent_loop = AgentLoop::new(
            inbound_rx,
            outbound_tx,
            inbound_tx,
            provider,
            config.workspace_path(),
            model,
            config.agents.defaults.max_tool_iterations,
            brave_key,
            config.tools.exec_.timeout,
            config.tools.exec_.restrict_to_workspace,
            Some(cron_service),
        );

        if let Some(msg) = message {
            let response = agent_loop
                .process_direct(&msg, &session_id, "cli", "direct")
                .await;
            println!("\n{} {}", LOGO, response);
        } else {
            println!("{} Interactive mode (Ctrl+C to exit)\n", LOGO);
            loop {
                print!("You: ");
                io::stdout().flush().ok();
                let mut input = String::new();
                match io::stdin().read_line(&mut input) {
                    Ok(0) | Err(_) => break,
                    _ => {}
                }
                let input = input.trim();
                if input.is_empty() {
                    continue;
                }
                let response = agent_loop
                    .process_direct(input, &session_id, "cli", "direct")
                    .await;
                println!("\n{} {}\n", LOGO, response);
            }
            println!("Goodbye!");
        }
    });
}

// ============================================================================
// Gateway
// ============================================================================

fn cmd_gateway(port: u16, verbose: bool) {
    if verbose {
        eprintln!("Verbose mode enabled");
    }

    println!("{} Starting nanobot gateway on port {}...", LOGO, port);

    let config = load_config(None);
    let api_key = config.get_api_key();
    let model = config.agents.defaults.model.clone();

    if api_key.is_none() && !model.starts_with("bedrock/") {
        eprintln!("Error: No API key configured.");
        std::process::exit(1);
    }

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    runtime.block_on(async {
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel::<InboundMessage>();
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<OutboundMessage>();

        let provider = create_provider(&config);
        let brave_key = if config.tools.web.search.api_key.is_empty() {
            None
        } else {
            Some(config.tools.web.search.api_key.clone())
        };

        let cron_store_path = get_data_dir().join("cron").join("jobs.json");
        let mut cron_service = CronService::new(cron_store_path);
        cron_service.start().await;
        let cron_status = cron_service.status();
        let cron_arc = Arc::new(cron_service);

        let mut agent_loop = AgentLoop::new(
            inbound_rx,
            outbound_tx,
            inbound_tx.clone(),
            provider,
            config.workspace_path(),
            model,
            config.agents.defaults.max_tool_iterations,
            brave_key,
            config.tools.exec_.timeout,
            config.tools.exec_.restrict_to_workspace,
            Some(cron_arc),
        );

        let channel_manager = ChannelManager::new(&config, inbound_tx, outbound_rx);

        let enabled = channel_manager.enabled_channels();
        if !enabled.is_empty() {
            println!("  Channels enabled: {}", enabled.join(", "));
        } else {
            println!("  Warning: No channels enabled");
        }

        {
            let job_count = cron_status.get("jobs").and_then(|v| v.as_i64()).unwrap_or(0);
            if job_count > 0 {
                println!("  Cron: {} scheduled jobs", job_count);
            }
        }

        println!("  Heartbeat: every 30m");

        tokio::select! {
            _ = agent_loop.run() => {
                info!("Agent loop ended");
            }
            _ = channel_manager.start_all() => {
                info!("Channel manager ended");
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down...");
            }
        }

        agent_loop.stop();
        channel_manager.stop_all().await;
    });
}

// ============================================================================
// Status
// ============================================================================

fn cmd_status() {
    let config_path = get_config_path();
    let config = load_config(None);
    let workspace = config.workspace_path();

    println!("{} nanobot Status\n", LOGO);
    println!(
        "Config: {} [{}]",
        config_path.display(),
        if config_path.exists() { "ok" } else { "missing" }
    );
    println!(
        "Workspace: {} [{}]",
        workspace.display(),
        if workspace.exists() { "ok" } else { "missing" }
    );

    if config_path.exists() {
        println!("Model: {}", config.agents.defaults.model);
        println!(
            "OpenRouter API: {}",
            if config.providers.openrouter.api_key.is_empty() { "not set" } else { "configured" }
        );
        println!(
            "Anthropic API: {}",
            if config.providers.anthropic.api_key.is_empty() { "not set" } else { "configured" }
        );
        println!(
            "OpenAI API: {}",
            if config.providers.openai.api_key.is_empty() { "not set" } else { "configured" }
        );
        println!(
            "Gemini API: {}",
            if config.providers.gemini.api_key.is_empty() { "not set" } else { "configured" }
        );
        let vllm_status = if let Some(ref base) = config.providers.vllm.api_base {
            format!("configured ({})", base)
        } else {
            "not set".to_string()
        };
        println!("vLLM/Local: {}", vllm_status);
    }
}

// ============================================================================
// Channels
// ============================================================================

fn cmd_channels_status() {
    let config = load_config(None);
    println!("Channel Status\n");
    println!(
        "  WhatsApp: {} ({})",
        if config.channels.whatsapp.enabled { "enabled" } else { "disabled" },
        config.channels.whatsapp.bridge_url
    );
    let tg_info = if config.channels.telegram.token.is_empty() {
        "not configured".to_string()
    } else {
        let t = &config.channels.telegram.token;
        format!("token: {}...", &t[..t.len().min(10)])
    };
    println!(
        "  Telegram: {} ({})",
        if config.channels.telegram.enabled { "enabled" } else { "disabled" },
        tg_info
    );
    println!(
        "  Feishu: {}",
        if config.channels.feishu.enabled { "enabled" } else { "disabled" }
    );
}

// ============================================================================
// Cron
// ============================================================================

fn cmd_cron_list(include_all: bool) {
    let store_path = get_data_dir().join("cron").join("jobs.json");
    let service = CronService::new(store_path);
    let jobs = service.list_jobs(include_all);

    if jobs.is_empty() {
        println!("No scheduled jobs.");
        return;
    }

    println!("Scheduled Jobs\n");
    println!(
        "{:<10} {:<20} {:<15} {:<10} {}",
        "ID", "Name", "Schedule", "Status", "Next Run"
    );
    println!("{}", "-".repeat(70));

    for job in &jobs {
        let sched = match job.schedule.kind.as_str() {
            "every" => format!("every {}s", job.schedule.every_ms.unwrap_or(0) / 1000),
            "cron" => job.schedule.expr.clone().unwrap_or_default(),
            _ => "one-time".to_string(),
        };
        let status = if job.enabled { "enabled" } else { "disabled" };
        let next_run = job
            .state
            .next_run_at_ms
            .map(|ms| {
                chrono::DateTime::from_timestamp(ms / 1000, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        println!(
            "{:<10} {:<20} {:<15} {:<10} {}",
            job.id, job.name, sched, status, next_run
        );
    }
}

fn cmd_cron_add(
    name: String,
    message: String,
    every: Option<u64>,
    cron_expr: Option<String>,
    deliver: bool,
    to: Option<String>,
    channel: Option<String>,
) {
    let schedule = if let Some(secs) = every {
        CronSchedule {
            kind: "every".to_string(),
            every_ms: Some((secs * 1000) as i64),
            ..Default::default()
        }
    } else if let Some(expr) = cron_expr {
        CronSchedule {
            kind: "cron".to_string(),
            expr: Some(expr),
            ..Default::default()
        }
    } else {
        eprintln!("Error: Must specify --every or --cron");
        std::process::exit(1);
    };

    let store_path = get_data_dir().join("cron").join("jobs.json");
    let mut service = CronService::new(store_path);
    let job = service.add_job(
        &name,
        schedule,
        &message,
        deliver,
        channel.as_deref(),
        to.as_deref(),
        false,
    );
    println!("  Added job '{}' ({})", job.name, job.id);
}

fn cmd_cron_remove(job_id: String) {
    let store_path = get_data_dir().join("cron").join("jobs.json");
    let mut service = CronService::new(store_path);
    if service.remove_job(&job_id) {
        println!("  Removed job {}", job_id);
    } else {
        eprintln!("Job {} not found", job_id);
    }
}

fn cmd_cron_enable(job_id: String, disable: bool) {
    let store_path = get_data_dir().join("cron").join("jobs.json");
    let mut service = CronService::new(store_path);
    if let Some(job) = service.enable_job(&job_id, !disable) {
        let status = if disable { "disabled" } else { "enabled" };
        println!("  Job '{}' {}", job.name, status);
    } else {
        eprintln!("Job {} not found", job_id);
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn create_provider(config: &Config) -> Arc<dyn LLMProvider> {
    let api_key = config.get_api_key().unwrap_or_default();
    let api_base = config.get_api_base();
    let model = &config.agents.defaults.model;
    Arc::new(OpenAICompatProvider::new(
        &api_key,
        api_base.as_deref(),
        Some(model.as_str()),
    ))
}
