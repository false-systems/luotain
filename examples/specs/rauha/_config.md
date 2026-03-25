# Rauha — Zone-Based Container Runtime

Isolation-first container runtime. Zones are the primary isolation primitive — each zone gets its own network identity on the `10.89.0.0/16` subnet.

The daemon (`rauhad`) must be running before tests. Tests use the `rauha` CLI to interact with the system.

## Connection

```toml
[target]
type = "cli"
command = "rauha"
```
