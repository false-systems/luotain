use clap::{Parser, Subcommand};
use luotain_core::http::HttpProbe;
use luotain_core::probe::ProbeRequest;
use luotain_core::registry::ProbeRegistry;
use luotain_core::spec::SpecTree;
use luotain_judge::agent::{Agent, AgentConfig};
use luotain_judge::anthropic::AnthropicProvider;
use luotain_judge::openai::OpenAiProvider;
use luotain_judge::provider::JudgeProvider;
use luotain_runner::runner::{RunConfig, Runner};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "luotain", version, about = "Blackbox probe toolkit for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show the spec tree structure
    Tree {
        /// Path to spec tree root
        spec_root: PathBuf,
    },
    /// Read a spec file
    Read {
        /// Path to spec tree root
        spec_root: PathBuf,
        /// Relative path to spec file within the tree
        path: String,
    },
    /// Execute a single probe against a live system
    Probe {
        #[command(subcommand)]
        probe_type: ProbeType,
    },
    /// Run all specs against a target — the full blackbox test pipeline
    Run {
        /// Path to spec tree root
        #[arg(long)]
        specs: PathBuf,
        /// Target base URL — overrides _config.md (e.g., http://localhost:8080)
        #[arg(long)]
        target: Option<String>,
        /// Environment name for config overrides (e.g., staging, prod)
        #[arg(long)]
        env: Option<String>,
        /// LLM provider: "anthropic" or "openai" (for any OpenAI-compatible API)
        #[arg(long, default_value = "anthropic")]
        provider: String,
        /// Model name (default: claude-sonnet-4-6 for anthropic, gpt-4o for openai)
        #[arg(long)]
        model: Option<String>,
        /// API base URL (for openai provider: Ollama, Groq, OpenRouter, etc.)
        #[arg(long)]
        base_url: Option<String>,
        /// API key (defaults to ANTHROPIC_API_KEY or OPENAI_API_KEY env var)
        #[arg(long)]
        api_key: Option<String>,
        /// Max probes per spec (cost control)
        #[arg(long, default_value = "50")]
        max_probes: usize,
        /// Max LLM turns per spec
        #[arg(long, default_value = "25")]
        max_turns: usize,
        /// Output format: "json" (stdout) or "pretty" (human-readable)
        #[arg(long, default_value = "pretty")]
        format: String,
    },
}

#[derive(Subcommand)]
enum ProbeType {
    /// HTTP probe
    Http {
        /// HTTP method (GET, POST, etc.)
        method: String,
        /// URL to probe
        url: String,
        /// Request body (JSON string)
        #[arg(long)]
        body: Option<String>,
        /// Headers as key:value (repeatable)
        #[arg(long = "header", short = 'H')]
        headers: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Tree { spec_root } => {
            let tree = SpecTree::open(&spec_root)?;
            let root = tree.walk()?;
            println!("{}", serde_json::to_string_pretty(&root)?);
        }
        Commands::Read { spec_root, path } => {
            let tree = SpecTree::open(&spec_root)?;
            let content = tree.read_spec(&path)?;
            print!("{}", content);
        }
        Commands::Probe { probe_type } => match probe_type {
            ProbeType::Http {
                method,
                url,
                body,
                headers,
            } => {
                let mut header_map = HashMap::new();
                for h in &headers {
                    if let Some((k, v)) = h.split_once(':') {
                        header_map.insert(k.trim().to_string(), v.trim().to_string());
                    }
                }
                let request = ProbeRequest {
                    method: method.to_uppercase(),
                    url,
                    headers: header_map,
                    body,
                };
                let probe = HttpProbe::new();
                let result = probe.probe(&request).await?;
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        },
        Commands::Run {
            specs,
            target,
            env,
            provider,
            model,
            base_url,
            api_key,
            max_probes,
            max_turns,
            format,
        } => {
            let judge_provider: Box<dyn JudgeProvider> = match provider.as_str() {
                "anthropic" => {
                    let key = api_key
                        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Set ANTHROPIC_API_KEY or pass --api-key"
                            )
                        })?;
                    Box::new(AnthropicProvider::new(
                        key,
                        model.unwrap_or_else(|| "claude-sonnet-4-6".into()),
                    ))
                }
                "openai" => {
                    let url = base_url
                        .unwrap_or_else(|| "https://api.openai.com/v1".into());
                    let key = api_key.or_else(|| std::env::var("OPENAI_API_KEY").ok());
                    Box::new(OpenAiProvider::new(
                        url,
                        model.unwrap_or_else(|| "gpt-4o".into()),
                        key,
                    ))
                }
                other => {
                    anyhow::bail!(
                        "Unknown provider: '{}'. Use 'anthropic' or 'openai'.",
                        other
                    );
                }
            };

            // Build probe registry — all probe types available
            let mut registry = ProbeRegistry::new();
            registry.register(Arc::new(HttpProbe::new()));
            registry.register(Arc::new(luotain_core::cli_probe::CliProbe));
            registry.register(Arc::new(luotain_core::tcp_probe::TcpProbe));

            let agent = Agent::new(
                judge_provider,
                registry,
                AgentConfig {
                    max_turns,
                    max_probes,
                },
            );

            let runner = Runner::new(agent);
            let run_config = RunConfig {
                spec_root: specs.to_string_lossy().to_string(),
                target_override: target,
                env,
            };

            let report = runner.run(&run_config).await?;

            match format.as_str() {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
                _ => {
                    report.print_summary();
                }
            }

            std::process::exit(report.exit_code());
        }
    }

    Ok(())
}
