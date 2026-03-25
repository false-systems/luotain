# AHTI — Correlation Engine

gRPC ingest + HTTP query API.

## Connection

```toml
[target]
type = "http"
base_url = "${AHTI_HTTP_ENDPOINT}"
```

## Auth

```toml
[auth]
type = "bearer"
token = "${AHTI_AUTH_TOKEN}"
```

## Environments

```toml
[env.local]
target.base_url = "http://localhost:8080"

[env.staging]
target.base_url = "https://ahti.staging.false.systems"
```
