# Luotain

Blackbox probe toolkit for AI agents. Test software without seeing the code.

Write behavior specs in markdown. Point an AI agent at a running system. Luotain reads specs, probes the system from the outside, and tells you what passes and what doesn't.

```
specs/                          luotain run
  auth/                         ── Luotain Report ──────────────
    login.md        ──────→       ✓ auth/login.md
    signup.md                     ✗ auth/signup.md
  payments/                       ✓ payments/checkout.md
    checkout.md                 ────────────────────────────────
```

## Quick start

### 1. Install

```sh
cargo install --path luotain-cli
```

### 2. Write a spec

Create `specs/health.md`:

```markdown
# Health Check

## GET /health
- Returns 200 OK
- Response is JSON with `status` field
```

### 3. Run it

```sh
# With Claude (default — needs ANTHROPIC_API_KEY)
luotain run --specs ./specs --target http://localhost:8080

# With a free local model via Ollama
luotain run --specs ./specs --target http://localhost:8080 \
  --provider openai --base-url http://localhost:11434/v1 --model llama3
```

That's it. Luotain reads your spec, probes the system, and tells you if it passes.

## Using with Claude Code (MCP)

No API key needed — Claude Code is the agent.

```sh
# Build the MCP server
cargo build --release -p luotain-mcp

# Register with Claude Code
claude mcp add luotain -- /path/to/luotain-mcp \
  --spec-root ./specs --target http://localhost:8080
```

Start a new Claude Code session and say: *"test the specs against localhost"*

Claude reads your specs, probes the system using Luotain's tools, and reports what passes.

## Supported models

Luotain works with any LLM that supports tool use.

**Anthropic (default)**
```sh
export ANTHROPIC_API_KEY=sk-ant-...
luotain run --specs ./specs
```

**OpenAI**
```sh
export OPENAI_API_KEY=sk-...
luotain run --specs ./specs --provider openai --model gpt-4o
```

**Ollama (free, local)**
```sh
ollama pull llama3
luotain run --specs ./specs --provider openai \
  --base-url http://localhost:11434/v1 --model llama3
```

**Groq (free tier, fast)**
```sh
luotain run --specs ./specs --provider openai \
  --base-url https://api.groq.com/openai/v1 --model mixtral-8x7b
```

**Any OpenAI-compatible API** — OpenRouter, Together, vLLM, LM Studio — just set `--base-url` and `--model`.

## Writing specs

Specs are plain markdown. No special syntax. The agent reads natural language and figures out what to test.

```markdown
# Login API

## POST /login
- Accepts JSON with `email` and `password`
- Returns 200 with `token` on valid credentials
- Returns 401 with `error` on bad credentials
- Rate limits to 5 attempts per minute
```

Organize specs in a directory tree that mirrors your software:

```
specs/
  myapp/
    _config.md            ← target URL, auth, environment overrides
    auth/
      login.md
      signup.md
    payments/
      checkout.md
    health/
      readiness.md
```

Each `.md` file is one testable unit. Add a file, you added a test.

### Config

Put a `_config.md` at the root of your spec tree to configure the target:

```markdown
# My API

```toml
[target]
type = "http"
base_url = "https://api.example.com"

[auth]
type = "bearer"
token = "${API_TOKEN}"

[env.staging]
target.base_url = "https://staging.api.example.com"
```
```

Secrets use `${VAR}` — resolved from environment variables. Use `--env staging` to switch environments.

### Running specific specs

```sh
# All specs for a product
luotain run --specs specs/myapp

# Just the auth specs
luotain run --specs specs/myapp --only auth/

# Single spec
luotain run --specs specs/myapp --only auth/login.md

# JSON output for CI
luotain run --specs specs/myapp --format json
```

Exit code: `0` if all specs pass, `1` if any fail.

## Probe types

Luotain gives the agent three ways to interact with your system:

| Probe | What it does | Example use |
|---|---|---|
| `probe_http` | Send HTTP requests, inspect responses | API testing, health checks |
| `probe_cli` | Run commands, check exit codes + output | CLI tools, migrations, container exec |
| `probe_tcp_connect` / `probe_tcp_send` | Test TCP connectivity, send raw bytes | Port checks, Redis PING, protocol probing |

Configure the connection type in `_config.md`:

```toml
# HTTP (default)
[target]
type = "http"
base_url = "http://localhost:8080"

# CLI
[target]
type = "cli"
command = "docker exec myapp"

# TCP
[target]
type = "tcp"
host = "localhost"
port = 6379
```

## CLI reference

```sh
luotain tree ./specs                    # Show spec tree
luotain read ./specs auth/login.md      # Read a spec
luotain probe http GET https://url      # Single HTTP probe
luotain run --specs ./specs             # Run all specs
luotain run --specs ./specs --only auth/ --env staging --format json
```

## Architecture

```
luotain-core     Spec tree, config, probe engine (HTTP, CLI, TCP)
luotain-judge    LLM agent loop, Anthropic + OpenAI-compatible providers
luotain-runner   Test run orchestration, reporting
luotain-mcp      MCP server for interactive agent use
luotain-cli      CLI binary
fp               FALSE Protocol integration
```

## Part of False Systems

Luotain is part of the [False Systems](https://github.com/false-systems) infrastructure toolkit:

- **SYKLI** — CI/CD engine. Runs Luotain as a pipeline task.
- **AHTI** — Correlation engine. Correlates probe results with infrastructure events.
- **FALSE Protocol** — Every probe emits an occurrence (`probe.*` namespace) that AHTI can track.

## License

Apache-2.0
