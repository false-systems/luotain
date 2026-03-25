# RAUTA — Gateway API

HTTP admin API for the gateway.

## Connection

```toml
[target]
type = "http"
base_url = "${RAUTA_ADMIN_ENDPOINT}"
```

## Environments

```toml
[env.local]
target.base_url = "http://localhost:9091"
```
