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
cargo test -p luotain-core                    # single crate
cargo test -p luotain-core spec::tests::test_walk_spec_tree  # single test
```

## Run

```sh
# Spec tree inspection
cargo run --bin luotain -- tree examples/specs/httpbin
cargo run --bin luotain -- read examples/specs/httpbin basics/get.md

# Single HTTP probe
cargo run --bin luotain -- probe http GET https://httpbin.org/get

# Full automated test run (needs ANTHROPIC_API_KEY or --api-key)
cargo run --bin luotain -- run --specs examples/specs/httpbin --target https://httpbin.org
cargo run --bin luotain -- run --specs ./specs --target http://localhost:8080 --format json

# With free/local models via OpenAI-compatible API
cargo run --bin luotain -- run --specs ./specs --target http://localhost:8080 \
  --provider openai --base-url http://localhost:11434/v1 --model llama3

# MCP server (stdio transport, for Claude Code / Cursor)
cargo run --bin luotain-mcp -- --spec-root examples/specs/httpbin --target https://httpbin.org
```

## Architecture (DDD Bounded Contexts)

Dependencies point inward: Runner → Judge → Core. Never the reverse.

**Observation Context (`luotain-core`)** — Stable domain core. Spec tree walker, probe types, HTTP probe engine, session management. No LLM awareness. Changes rarely.

**Judgment Context (`luotain-judge`)** — AI-native boundary. The `JudgeProvider` trait is an anti-corruption layer isolating the domain from LLM wire formats. The `Agent` entity drives one lifecycle per spec: read → probe → observe → verdict. The system prompt in `prompt.rs` is business logic — treat changes to it like code changes.
- `types.rs`: `Turn`, `ToolCall`, `ToolResult`, `AssistantMessage` — abstractions over both Anthropic and OpenAI message formats
- `anthropic.rs` / `openai.rs`: Wire format adapters implementing `JudgeProvider`
- `agent.rs`: The agent loop — iterates LLM turns, executes tool calls (`probe_http`, `record_verdict`), accumulates evidence, produces `SpecResult`

**Orchestration Context (`luotain-runner`)** — Thin coordination. Walks spec tree, invokes judge agent per spec, assembles `TestReport`. Exit code: 0 = all pass, 1 = failures.

**Integration Context (`fp`)** — FALSE Protocol type mappings. Occurrence namespace: `probe.*` (probe.http.response, probe.session.completed, probe.verdict.recorded).

**MCP Server (`luotain-mcp`)** — Stdio JSON-RPC server for interactive mode. 5 tools: `luotain_list_specs`, `luotain_read_spec`, `luotain_probe_http`, `luotain_record_verdict`, `luotain_report`. Hand-rolled MCP protocol (no rmcp dependency).

**CLI (`luotain-cli`)** — Two binaries: `luotain` (CLI) and `luotain-mcp` (MCP server).

## Conventions

- No `.unwrap()` in production code — use `?` or handle errors
- No `println!` in library code — use `tracing` (logs go to stderr in CLI)
- `thiserror` for library error types, `anyhow` for binary entry points
- Workspace lints enforced: `unsafe_code = "deny"`, `unwrap_used = "warn"`

## FALSE Protocol Type Namespace

Type prefix: `probe.*`. Types must match `^[a-z0-9]+(\.[a-z0-9]+)+$` (min 2 segments).

## Spec Format

Specs are pure markdown — no frontmatter, no structured parsing. The agent interprets natural language. The directory tree mirrors the software under test. Each `.md` file is one testable unit.

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
