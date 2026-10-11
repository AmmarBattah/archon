//! Aether's `mcp_servers` reply, narrowed to one server's health.

use archon_core::error::ArchonError;
use archon_core::protocol::McpHealth;
use serde_json::Value;

/// The health of the server `name` in an `mcp_servers` reply. A state newer
/// than this client reads as `Unknown`, as Aether's own `unknown` does.
pub fn server_health(servers: &Value, name: &str) -> Result<McpHealth, ArchonError> {
    assert!(!name.is_empty(), "a server is looked up by name");
    let Some(list) = servers.as_array() else {
        return Err(ArchonError::Protocol(format!(
            "mcp_servers replied with a non-list: {servers}"
        )));
    };
    let named = |s: &&Value| s.get("name").and_then(Value::as_str) == Some(name);
    let Some(server) = list.iter().find(named) else {
        return Ok(McpHealth::NotLoaded);
    };
    let text = |key: &str| server.get(key).and_then(Value::as_str);
    let Some(state) = text("state") else {
        return Err(ArchonError::Protocol(format!(
            "mcp_servers gave the {name} server no state"
        )));
    };
    let health = match state {
        "ready" => {
            let tools = server
                .get("tools")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            McpHealth::Ready {
                tools: u32::try_from(tools).unwrap_or(u32::MAX),
            }
        }
        "failed" => {
            let error = text("error").filter(|e| !e.trim().is_empty());
            McpHealth::Failed {
                error: error.unwrap_or("Aether gave no reason").to_owned(),
            }
        }
        _ => McpHealth::Unknown,
    };
    Ok(health)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn each_aether_state_maps_to_a_health() {
        let ready = json!([{"name": "other", "state": "failed"},
                           {"name": "archon", "state": "ready", "tools": [{}, {}]}]);
        let health = server_health(&ready, "archon").unwrap();
        assert_eq!(health, McpHealth::Ready { tools: 2 });
        let failed = json!([{"name": "archon", "state": "failed", "error": "Broken pipe"}]);
        let error = "Broken pipe".to_owned();
        assert_eq!(
            server_health(&failed, "archon").unwrap(),
            McpHealth::Failed { error }
        );
        let silent = json!([{"name": "archon", "state": "failed"}]);
        assert!(matches!(
            server_health(&silent, "archon").unwrap(),
            McpHealth::Failed { error } if !error.is_empty()
        ));
        let unknown = json!([{"name": "archon", "state": "unknown"}]);
        assert_eq!(
            server_health(&unknown, "archon").unwrap(),
            McpHealth::Unknown
        );
        let newer = json!([{"name": "archon", "state": "draining"}]);
        assert_eq!(server_health(&newer, "archon").unwrap(), McpHealth::Unknown);
    }

    #[test]
    fn a_missing_server_is_not_loaded_and_a_malformed_reply_is_refused() {
        let none = server_health(&json!([]), "archon").unwrap();
        assert_eq!(none, McpHealth::NotLoaded);
        let stateless = json!([{"name": "archon"}]);
        assert!(matches!(
            server_health(&stateless, "archon"),
            Err(ArchonError::Protocol(_))
        ));
        assert!(matches!(
            server_health(&json!({}), "archon"),
            Err(ArchonError::Protocol(_))
        ));
    }
}
