# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What is Luotain

Blackbox probe toolkit for AI agents. Part of the False Systems ecosystem (AHTI, SYKLI, RAUTA, POLKU, etc.).

Luotain probes running software from the outside — no source code access. Behavior specs are markdown files in a directory tree. An AI agent reads specs, uses Luotain to probe the live system, and verifies behavior matches.

Two operating modes:
- **Interactive (MCP):** Claude Code uses Luotain MCP tools to read specs, probe, and record verdicts directly
- **Automated (CLI runner):** `luotain run` drives an LLM agent loop that reads specs, probes, judges, and reports — for CI/SYKLI integration

## Build & Test

```sh
cargo build
cargo test
cargo test -p luotain-core                                     # single crate
cargo test -p luotain-core config::tests::test_env_override    # single test
```

## Run

```sh
# Spec tree inspection
cargo run --bin luotain -- tree examples/specs/httpbin
cargo run --bin luotain -- read examples/specs/httpbin basics/get.md

# Single HTTP probe
cargo run --bin luotain -- probe http GET https://httpbin.org/get

# Full automated test run — reads _config.md from spec root
cargo run --bin luotain -- run --specs examples/specs/httpbin

# With explicit target (overrides _config.md)
cargo run --bin luotain -- run --specs ./specs --target http://localhost:8080

# With environment override
cargo run --bin luotain -- run --specs ./specs --env prod

# JSON output for CI
cargo run --bin luotain -- run --specs ./specs --format json

# With free/local models via OpenAI-compatible API
cargo run --bin luotain -- run --specs ./specs \
  --provider openai --base-url http://localhost:11434/v1 --model llama3

# MCP server (stdio transport, for Claude Code / Cursor)
cargo run --bin luotain-mcp -- --spec-root examples/specs/httpbin --target https://httpbin.org
```

## Architecture (DDD Bounded Contexts)

Dependencies point inward: Runner → Judge → Core. Never the reverse.

**Observation Context (`luotain-core`)** — Stable domain core. Changes rarely.
- **Spec tree:** `spec.rs` walks directories, filters `_config.md` from listings
- **Config:** `config.rs` parses `_config.md` (TOML blocks in markdown), resolves `${VAR}` secrets from env vars, applies `--env` overrides. Types: `TargetConfig`, `Connection` (Http/Grpc/Tcp/Cli), `Auth` (Bearer/Basic/ApiKey/Mtls)
- **Probe plugin system:** `probe_trait.rs` defines the `Probe` trait; `registry.rs` maps tool names to `Arc<dyn Probe>` implementations; `tool.rs` defines `ToolDef` (the domain-side tool definition, mapped to judge's `Tool` at the boundary)
- **Probe types:** `http.rs` (HttpProbe), `cli_probe.rs` (CliProbe — run commands, capture stdout/stderr/exit code), `tcp_probe.rs` (TcpProbe — probe_tcp_connect + probe_tcp_send)
- **Observation:** `probe.rs` has a flat `Observation` struct with optional fields per probe type (`status` for HTTP, `exit_code`/`stderr` for CLI, `tls_established` for TCP). `ProbeKind` enum tags which probe generated it. `skip_serializing_if` keeps the agent's JSON clean.

**Judgment Context (`luotain-judge`)** — AI-native boundary.
- `provider.rs`: `JudgeProvider` trait — anti-corruption layer isolating the domain from LLM wire formats
- `anthropic.rs` / `openai.rs`: Wire format adapters (Anthropic Messages API / OpenAI Chat Completions)
- `types.rs`: `Turn`, `ToolCall`, `ToolResult`, `AssistantMessage` — abstractions over both formats
- `agent.rs`: The agent loop. Gets tool definitions from `ProbeRegistry` dynamically. Dispatches tool calls through the registry. `record_verdict` is always available (not a probe). Takes `TargetConfig` and passes it to probes for auth.
- `prompt.rs`: System prompt — **treat changes like business logic**. Dynamic: lists available probe tools based on what's registered.

**Orchestration Context (`luotain-runner`)** — Thin coordination. Walks spec tree, loads `_config.md`, resolves target (CLI `--target` > `_config.md` > error), invokes judge agent per spec, assembles `TestReport`. Exit code: 0 = all pass, 1 = failures.

**Integration Context (`fp`)** — FALSE Protocol type mappings. Occurrence namespace: `probe.*`.

**MCP Server (`luotain-mcp`)** — Stdio JSON-RPC server for interactive mode. 5 tools: `luotain_list_specs`, `luotain_read_spec`, `luotain_probe_http`, `luotain_record_verdict`, `luotain_report`. Hand-rolled MCP protocol.

**CLI (`luotain-cli`)** — Two binaries: `luotain` (CLI) and `luotain-mcp` (MCP server). Builds probe registry with all available probe types.

## Adding a New Probe Type

1. Create `luotain-core/src/my_probe.rs` implementing the `Probe` trait (kind, tool_definitions, execute)
2. Add `pub mod my_probe;` to `luotain-core/src/lib.rs`
3. Register in `luotain-cli/src/main.rs`: `registry.register(Arc::new(MyProbe))`
4. Heavy dependencies (tonic, sqlx) should be feature-gated in `Cargo.toml`

## Config System

`_config.md` at spec tree root. TOML fenced code blocks inside markdown. The agent can read the prose as context; the system parses the TOML blocks mechanically.

- `${VAR}` for secrets — resolved from env vars at load time
- `[env.staging]` / `[env.prod]` sections for environment-specific overrides
- `--target URL` CLI flag overrides the config's connection (but auth is still loaded)
- No `_config.md`? Then `--target` is required.

## Conventions

- No `.unwrap()` in production code — use `?` or handle errors
- No `println!` in library code — use `tracing` (logs go to stderr in CLI)
- `thiserror` for library error types, `anyhow` for binary entry points
- Workspace lints enforced: `unsafe_code = "deny"`, `unwrap_used = "warn"`

## SYKLI Integration

Luotain integrates with SYKLI as a command-based task:
```rust
p.task("blackbox-test")
    .run("luotain run --specs /specs --target $TARGET_URL --format json")
    .covers(&["specs/**/*.md"])
    .intent("Blackbox behavior validation")
    .critical()
    .on_fail(OnFailAction::Analyze);
```
Exit code 0/1 maps to SYKLI's passed/failed. JSON stdout is captured as task output.

## FALSE Protocol Type Namespace

Type prefix: `probe.*`. Types must match `^[a-z0-9]+(\.[a-z0-9]+)+$` (min 2 segments).
