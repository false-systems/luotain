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

## How it works

1. You write behavior specs as markdown files:

```markdown
# Login API

## POST /login
- Accepts JSON with `email` and `password`
- Returns 200 with `token` on valid credentials
- Returns 401 with `error` on bad credentials
- Rate limits to 5 attempts per minute
```

2. The directory tree mirrors the software you're testing
3. An AI agent reads each spec, probes the live system, and produces a verdict

No test code. No assertions. No fixtures. Just markdown describing what the software should do.

## Two modes

**Interactive** — Add Luotain as an MCP server to Claude Code. The agent reads specs and probes the system using tools directly.

```sh
luotain-mcp --spec-root ./specs --target http://localhost:8080
```

**Automated** — Run from CLI or CI. Luotain drives an LLM agent loop internally.

```sh
luotain run --specs ./specs --target http://localhost:8080
```

## Install

```sh
cargo install --path luotain-cli
```

## Usage

```sh
# See what specs exist
luotain tree ./specs

# Read a spec
luotain read ./specs auth/login.md

# Probe a system directly
luotain probe http GET https://httpbin.org/get
luotain probe http POST https://httpbin.org/post --body '{"key":"value"}'

# Run all specs against a target (needs ANTHROPIC_API_KEY)
luotain run --specs ./specs --target http://localhost:8080

# JSON output for CI
luotain run --specs ./specs --target http://localhost:8080 --format json

# Use any OpenAI-compatible model (Ollama, Groq, OpenRouter, etc.)
luotain run --specs ./specs --target http://localhost:8080 \
  --provider openai \
  --base-url http://localhost:11434/v1 \
  --model llama3
```

Exit code: `0` if all specs pass, `1` if any fail.

## Spec format

Specs are plain markdown. No frontmatter, no special syntax. The agent reads natural language and figures out what to test.

```
specs/
  myapp/
    auth/
      login.md
      signup.md
      rate-limiting.md
    payments/
      checkout.md
      refunds.md
    health/
      readiness.md
```

Each `.md` file is one testable unit. The directory structure is the test structure.

## Architecture

```
luotain-core     Types, spec tree walker, HTTP probe engine
luotain-judge    LLM agent loop, Anthropic + OpenAI-compatible providers
luotain-runner   Test run orchestration, reporting
luotain-mcp      MCP server for interactive agent use
luotain-cli      CLI binary
fp               FALSE Protocol integration
```

## Part of False Systems

Luotain is part of the [False Systems](https://false.systems) infrastructure toolkit:

- **SYKLI** — CI/CD engine. Runs Luotain as a pipeline task.
- **AHTI** — Correlation engine. Correlates probe results with infrastructure events.
- **FALSE Protocol** — Every probe emits an occurrence (`probe.*` namespace) that AHTI can track.

```rust
// SYKLI pipeline
p.task("blackbox-test")
    .run("luotain run --specs /specs --target $TARGET_URL --format json")
    .covers(&["specs/**/*.md"])
    .critical()
    .on_fail(OnFailAction::Analyze);
```

## License

Apache-2.0
