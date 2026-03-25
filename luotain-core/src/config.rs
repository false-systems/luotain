//! Config system -- how to reach and authenticate against the target system.
//!
//! Config lives in `_config.md` at the root of a spec tree. TOML blocks
//! inside markdown give us both human readability and machine parseability.
//! Secrets use `${VAR}` syntax -- resolved from env vars at load time.

use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// The config filename — excluded from spec listings.
pub const CONFIG_FILENAME: &str = "_config.md";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config parse error: {0}")]
    Parse(String),
    #[error("unresolved secret: ${{{0}}} — set this environment variable")]
    UnresolvedSecret(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("unknown connection type: {0}")]
    UnknownConnectionType(String),
    #[error("missing field: {0}")]
    MissingField(String),
}

/// Resolved target configuration — secrets expanded, environment applied.
#[derive(Debug, Clone)]
pub struct TargetConfig {
    pub connection: Connection,
    pub auth: Option<Auth>,
}

#[derive(Debug, Clone)]
pub enum Connection {
    Http {
        base_url: String,
    },
    Grpc {
        host: String,
        port: u16,
        tls: bool,
        proto_path: Option<PathBuf>,
    },
    Tcp {
        host: String,
        port: u16,
        tls: bool,
    },
    Cli {
        command: String,
        shell: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum Auth {
    Bearer { token: String },
    Basic { username: String, password: String },
    ApiKey { header: String, key: String },
    Mtls {
        cert_path: PathBuf,
        key_path: PathBuf,
        ca_path: Option<PathBuf>,
    },
}

/// Load config by searching for `_config.md`.
///
/// Looks in `spec_root` first, then walks upward through parent directories.
/// This means `specs/ahti/ingest` finds `specs/ahti/_config.md`.
/// Returns `Ok(None)` if no config file is found anywhere.
pub fn load_config(
    spec_root: &Path,
    env: Option<&str>,
) -> Result<Option<TargetConfig>, ConfigError> {
    if let Some(config_path) = find_config(spec_root) {
        let content = std::fs::read_to_string(&config_path)?;
        return parse_config(&content, env).map(Some);
    }
    Ok(None)
}

/// Search for `_config.md` starting at `dir` and walking upward.
pub fn find_config(dir: &Path) -> Option<std::path::PathBuf> {
    let mut current = dir.to_path_buf();
    loop {
        let candidate = current.join(CONFIG_FILENAME);
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Create a TargetConfig from a plain URL (for --target override).
pub fn config_from_url(url: String) -> TargetConfig {
    TargetConfig {
        connection: Connection::Http { base_url: url },
        auth: None,
    }
}

/// Parse config from markdown content.
fn parse_config(markdown: &str, env: Option<&str>) -> Result<TargetConfig, ConfigError> {
    let toml_content = extract_toml_blocks(markdown);
    if toml_content.is_empty() {
        return Err(ConfigError::Parse(
            "no ```toml blocks found in config".into(),
        ));
    }

    let raw: RawConfig =
        toml::from_str(&toml_content).map_err(ConfigError::Toml)?;

    // Apply environment override if specified
    let raw = if let Some(env_name) = env {
        apply_env_override(raw, env_name)
    } else {
        raw
    };

    resolve_config(raw)
}

/// Extract all ```toml ... ``` fenced code blocks and concatenate them.
fn extract_toml_blocks(markdown: &str) -> String {
    let mut blocks = Vec::new();
    let mut in_toml_block = false;
    let mut current_block = String::new();

    for line in markdown.lines() {
        if line.trim_start().starts_with("```toml") && !in_toml_block {
            in_toml_block = true;
            current_block.clear();
        } else if line.trim_start().starts_with("```") && in_toml_block {
            in_toml_block = false;
            blocks.push(current_block.clone());
        } else if in_toml_block {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    blocks.join("\n")
}

/// Resolve `${VAR}` references from environment variables.
fn resolve_secrets(input: &str) -> Result<String, ConfigError> {
    let re = Regex::new(r"\$\{([^}]+)\}").map_err(|e| ConfigError::Parse(e.to_string()))?;

    let mut result = input.to_string();
    for cap in re.captures_iter(input) {
        let var_name = &cap[1];
        let value = std::env::var(var_name)
            .map_err(|_| ConfigError::UnresolvedSecret(var_name.to_string()))?;
        result = result.replace(&cap[0], &value);
    }

    Ok(result)
}

/// Resolve optional string field with secret expansion.
fn resolve_optional(field: &Option<String>) -> Result<Option<String>, ConfigError> {
    match field {
        Some(s) => Ok(Some(resolve_secrets(s)?)),
        None => Ok(None),
    }
}

/// Apply environment overrides to the raw config.
fn apply_env_override(mut raw: RawConfig, env_name: &str) -> RawConfig {
    let overrides = raw
        .env
        .as_mut()
        .and_then(|envs| envs.remove(env_name));

    if let Some(ov) = overrides {
        if let Some(target_ov) = ov.target {
            let target = raw.target.get_or_insert_with(RawTarget::default);
            if target_ov.conn_type.is_some() {
                target.conn_type = target_ov.conn_type;
            }
            if target_ov.base_url.is_some() {
                target.base_url = target_ov.base_url;
            }
            if target_ov.host.is_some() {
                target.host = target_ov.host;
            }
            if target_ov.port.is_some() {
                target.port = target_ov.port;
            }
            if target_ov.tls.is_some() {
                target.tls = target_ov.tls;
            }
            if target_ov.proto_path.is_some() {
                target.proto_path = target_ov.proto_path;
            }
            if target_ov.command.is_some() {
                target.command = target_ov.command;
            }
            if target_ov.shell.is_some() {
                target.shell = target_ov.shell;
            }
        }
        if let Some(auth_ov) = ov.auth {
            let auth = raw.auth.get_or_insert_with(RawAuth::default);
            if auth_ov.auth_type.is_some() {
                auth.auth_type = auth_ov.auth_type;
            }
            if auth_ov.token.is_some() {
                auth.token = auth_ov.token;
            }
            if auth_ov.username.is_some() {
                auth.username = auth_ov.username;
            }
            if auth_ov.password.is_some() {
                auth.password = auth_ov.password;
            }
            if auth_ov.header.is_some() {
                auth.header = auth_ov.header;
            }
            if auth_ov.key.is_some() {
                auth.key = auth_ov.key;
            }
            if auth_ov.cert_path.is_some() {
                auth.cert_path = auth_ov.cert_path;
            }
            if auth_ov.key_path.is_some() {
                auth.key_path = auth_ov.key_path;
            }
            if auth_ov.ca_path.is_some() {
                auth.ca_path = auth_ov.ca_path;
            }
        }
    }

    raw
}

/// Convert raw parsed config into resolved TargetConfig.
fn resolve_config(raw: RawConfig) -> Result<TargetConfig, ConfigError> {
    let target = raw
        .target
        .ok_or_else(|| ConfigError::MissingField("target".into()))?;

    let conn_type = target.conn_type.as_deref().unwrap_or("http");

    let connection = match conn_type {
        "http" => {
            let base_url = target
                .base_url
                .ok_or_else(|| ConfigError::MissingField("target.base_url".into()))?;
            Connection::Http {
                base_url: resolve_secrets(&base_url)?,
            }
        }
        "grpc" => {
            let host = target
                .host
                .ok_or_else(|| ConfigError::MissingField("target.host".into()))?;
            Connection::Grpc {
                host: resolve_secrets(&host)?,
                port: target.port.unwrap_or(443),
                tls: target.tls.unwrap_or(true),
                proto_path: target.proto_path.map(PathBuf::from),
            }
        }
        "tcp" => {
            let host = target
                .host
                .ok_or_else(|| ConfigError::MissingField("target.host".into()))?;
            Connection::Tcp {
                host: resolve_secrets(&host)?,
                port: target
                    .port
                    .ok_or_else(|| ConfigError::MissingField("target.port".into()))?,
                tls: target.tls.unwrap_or(false),
            }
        }
        "cli" => {
            let command = target
                .command
                .ok_or_else(|| ConfigError::MissingField("target.command".into()))?;
            Connection::Cli {
                command: resolve_secrets(&command)?,
                shell: resolve_optional(&target.shell)?,
            }
        }
        other => return Err(ConfigError::UnknownConnectionType(other.into())),
    };

    let auth = match raw.auth {
        None => None,
        Some(raw_auth) => {
            let auth_type = raw_auth.auth_type.as_deref().unwrap_or("bearer");
            Some(match auth_type {
                "bearer" => {
                    let token = raw_auth
                        .token
                        .ok_or_else(|| ConfigError::MissingField("auth.token".into()))?;
                    Auth::Bearer {
                        token: resolve_secrets(&token)?,
                    }
                }
                "basic" => {
                    let username = raw_auth
                        .username
                        .ok_or_else(|| ConfigError::MissingField("auth.username".into()))?;
                    let password = raw_auth
                        .password
                        .ok_or_else(|| ConfigError::MissingField("auth.password".into()))?;
                    Auth::Basic {
                        username: resolve_secrets(&username)?,
                        password: resolve_secrets(&password)?,
                    }
                }
                "api_key" => {
                    let header = raw_auth
                        .header
                        .ok_or_else(|| ConfigError::MissingField("auth.header".into()))?;
                    let key = raw_auth
                        .key
                        .ok_or_else(|| ConfigError::MissingField("auth.key".into()))?;
                    Auth::ApiKey {
                        header: resolve_secrets(&header)?,
                        key: resolve_secrets(&key)?,
                    }
                }
                "mtls" => {
                    let cert = raw_auth
                        .cert_path
                        .ok_or_else(|| ConfigError::MissingField("auth.cert_path".into()))?;
                    let key = raw_auth
                        .key_path
                        .ok_or_else(|| ConfigError::MissingField("auth.key_path".into()))?;
                    Auth::Mtls {
                        cert_path: PathBuf::from(resolve_secrets(&cert)?),
                        key_path: PathBuf::from(resolve_secrets(&key)?),
                        ca_path: resolve_optional(&raw_auth.ca_path)?.map(PathBuf::from),
                    }
                }
                other => {
                    return Err(ConfigError::Parse(format!("unknown auth type: {}", other)))
                }
            })
        }
    };

    Ok(TargetConfig { connection, auth })
}

// --- Raw deserialization types ---

#[derive(Debug, Deserialize, Default)]
struct RawConfig {
    target: Option<RawTarget>,
    auth: Option<RawAuth>,
    env: Option<HashMap<String, RawEnvOverride>>,
}

#[derive(Debug, Deserialize, Default)]
struct RawTarget {
    #[serde(rename = "type")]
    conn_type: Option<String>,
    base_url: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    tls: Option<bool>,
    proto_path: Option<String>,
    command: Option<String>,
    shell: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawAuth {
    #[serde(rename = "type")]
    auth_type: Option<String>,
    token: Option<String>,
    username: Option<String>,
    password: Option<String>,
    header: Option<String>,
    key: Option<String>,
    cert_path: Option<String>,
    key_path: Option<String>,
    ca_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawEnvOverride {
    target: Option<RawTarget>,
    auth: Option<RawAuth>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_toml_blocks() {
        let md = r#"# Config

Some text.

```toml
[target]
type = "http"
base_url = "https://example.com"
```

More text.

```toml
[auth]
type = "bearer"
token = "static-token"
```
"#;
        let toml = extract_toml_blocks(md);
        assert!(toml.contains("[target]"));
        assert!(toml.contains("[auth]"));
    }

    #[test]
    fn test_parse_http_config() {
        let md = r#"
```toml
[target]
type = "http"
base_url = "https://httpbin.org"
```
"#;
        let config = parse_config(md, None).expect("should parse");
        match &config.connection {
            Connection::Http { base_url } => assert_eq!(base_url, "https://httpbin.org"),
            _ => panic!("expected HTTP connection"),
        }
        assert!(config.auth.is_none());
    }

    #[test]
    fn test_parse_grpc_config() {
        let md = r#"
```toml
[target]
type = "grpc"
host = "localhost"
port = 50051
tls = false
```
"#;
        let config = parse_config(md, None).expect("should parse");
        match &config.connection {
            Connection::Grpc {
                host, port, tls, ..
            } => {
                assert_eq!(host, "localhost");
                assert_eq!(*port, 50051);
                assert!(!tls);
            }
            _ => panic!("expected gRPC connection"),
        }
    }

    #[test]
    fn test_parse_cli_config() {
        let md = r#"
```toml
[target]
type = "cli"
command = "docker exec myapp"
shell = "bash"
```
"#;
        let config = parse_config(md, None).expect("should parse");
        match &config.connection {
            Connection::Cli { command, shell } => {
                assert_eq!(command, "docker exec myapp");
                assert_eq!(shell.as_deref(), Some("bash"));
            }
            _ => panic!("expected CLI connection"),
        }
    }

    #[test]
    fn test_parse_auth_bearer() {
        let md = r#"
```toml
[target]
type = "http"
base_url = "https://api.example.com"

[auth]
type = "bearer"
token = "my-secret-token"
```
"#;
        let config = parse_config(md, None).expect("should parse");
        match &config.auth {
            Some(Auth::Bearer { token }) => assert_eq!(token, "my-secret-token"),
            _ => panic!("expected bearer auth"),
        }
    }

    #[test]
    fn test_env_override() {
        let md = r#"
```toml
[target]
type = "http"
base_url = "https://staging.example.com"

[env.prod]
target.base_url = "https://api.example.com"
```
"#;
        let config = parse_config(md, Some("prod")).expect("should parse");
        match &config.connection {
            Connection::Http { base_url } => assert_eq!(base_url, "https://api.example.com"),
            _ => panic!("expected HTTP"),
        }
    }

    #[test]
    fn test_secret_resolution() {
        std::env::set_var("LUOTAIN_TEST_TOKEN", "resolved-value");
        let result = resolve_secrets("Bearer ${LUOTAIN_TEST_TOKEN}");
        assert_eq!(result.expect("should resolve"), "Bearer resolved-value");
        std::env::remove_var("LUOTAIN_TEST_TOKEN");
    }

    #[test]
    fn test_unresolved_secret_errors() {
        let result = resolve_secrets("${DEFINITELY_NOT_SET_12345}");
        assert!(result.is_err());
        match result.unwrap_err() {
            ConfigError::UnresolvedSecret(var) => {
                assert_eq!(var, "DEFINITELY_NOT_SET_12345")
            }
            other => panic!("expected UnresolvedSecret, got: {}", other),
        }
    }

    #[test]
    fn test_no_toml_blocks_errors() {
        let md = "# Just markdown\n\nNo config here.";
        let result = parse_config(md, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_from_url() {
        let config = config_from_url("https://httpbin.org".into());
        match &config.connection {
            Connection::Http { base_url } => assert_eq!(base_url, "https://httpbin.org"),
            _ => panic!("expected HTTP"),
        }
    }
}
