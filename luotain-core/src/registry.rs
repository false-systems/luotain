//! Probe registry — maps tool names to probe implementations.
//!
//! The runner builds a registry at startup. The agent receives tool
//! definitions from it and dispatches tool calls through it.

use crate::config::TargetConfig;
use crate::probe::ProbeResult;
use crate::probe_trait::{Probe, ProbeError};
use crate::tool::ToolDef;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry of available probe types.
pub struct ProbeRegistry {
    /// Map from tool name → probe implementation
    tools: HashMap<String, Arc<dyn Probe>>,
    /// All tool definitions (cached)
    definitions: Vec<ToolDef>,
}

impl ProbeRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            definitions: Vec::new(),
        }
    }

    /// Register a probe. Its tool definitions are indexed for dispatch.
    pub fn register(&mut self, probe: Arc<dyn Probe>) {
        let defs = probe.tool_definitions();
        for def in &defs {
            self.tools.insert(def.name.clone(), Arc::clone(&probe));
        }
        self.definitions.extend(defs);
    }

    /// All tool definitions for the agent.
    pub fn tool_definitions(&self) -> &[ToolDef] {
        &self.definitions
    }

    /// Check if a tool name is registered.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Dispatch a tool call to the right probe.
    pub async fn execute(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        target: &TargetConfig,
    ) -> Result<ProbeResult, ProbeError> {
        let probe = self
            .tools
            .get(tool_name)
            .ok_or_else(|| ProbeError::NotSupported(format!("unknown tool: {}", tool_name)))?;
        probe.execute(tool_name, input, target).await
    }
}

impl Default for ProbeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
