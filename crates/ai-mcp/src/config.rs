//! Emitters for `ai-mcp print-config` / `ai-mcp print-agents-md`.
//!
//! The rendered strings are the single source of truth for the committed
//! `integrations/` fixtures; the tests below assert byte-for-byte parity, so
//! the generator and the checked-in files can never drift apart.

use serde_json::{Map, Value};

/// The MCP client config wrapper shape to emit.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClientShape {
    /// `{"mcpServers": …}` — Claude Code/Desktop, Cursor, Windsurf, Cline, Zed.
    McpServers,
    /// `{"servers": {… "type": "stdio"}}` — VS Code (`.vscode/mcp.json`).
    VsCode,
}

/// The verbatim AGENTS.md guidance block (marker-wrapped, trailing newline).
pub const AGENTS_MD: &str = include_str!("../../../integrations/agents/AGENTS.wmaker-ai.md");

/// The disposable-desktop container image the sandbox invocation launches.
const SANDBOX_IMAGE: &str = "wmaker-ai-sandbox";

/// The default server name for each lane.
pub fn default_name(sandbox: bool) -> &'static str {
    if sandbox {
        "wmaker-sandbox"
    } else {
        "wmaker-desktop"
    }
}

/// `(command, args)` for the requested lane. `command_override` lets the CLI
/// substitute an absolute `ai-mcp` path (`--absolute`).
fn invocation(sandbox: bool, command_override: Option<&str>) -> (String, Vec<String>) {
    if sandbox {
        let args = ["run", "-i", "--rm", SANDBOX_IMAGE]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        (command_override.unwrap_or("docker").to_string(), args)
    } else {
        (command_override.unwrap_or("ai-mcp").to_string(), Vec::new())
    }
}

/// Render an MCP client config as pretty JSON with a trailing newline.
///
/// Keys serialize alphabetically (serde_json's default `Map` is a `BTreeMap`),
/// which is exactly how the committed `integrations/mcp/*.json` are written.
pub fn render_config(
    shape: ClientShape,
    sandbox: bool,
    name: &str,
    command_override: Option<&str>,
) -> String {
    let (command, args) = invocation(sandbox, command_override);

    let mut server = Map::new();
    server.insert("args".to_string(), Value::from(args));
    server.insert("command".to_string(), Value::from(command));
    if shape == ClientShape::VsCode {
        server.insert("type".to_string(), Value::from("stdio"));
    }

    let mut servers = Map::new();
    servers.insert(name.to_string(), Value::Object(server));

    let root_key = match shape {
        ClientShape::McpServers => "mcpServers",
        ClientShape::VsCode => "servers",
    };
    let mut root = Map::new();
    root.insert(root_key.to_string(), Value::Object(servers));

    let mut out = serde_json::to_string_pretty(&Value::Object(root)).expect("config serializes");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_mcpservers_matches_committed_fixture() {
        assert_eq!(
            render_config(ClientShape::McpServers, false, default_name(false), None),
            include_str!("../../../integrations/mcp/mcp.json"),
        );
    }

    #[test]
    fn sandbox_mcpservers_matches_committed_fixture() {
        assert_eq!(
            render_config(ClientShape::McpServers, true, default_name(true), None),
            include_str!("../../../integrations/mcp/mcp.sandbox.json"),
        );
    }

    #[test]
    fn vscode_matches_committed_fixture() {
        assert_eq!(
            render_config(ClientShape::VsCode, false, default_name(false), None),
            include_str!("../../../integrations/mcp/vscode-mcp.json"),
        );
    }

    #[test]
    fn absolute_command_override_is_honored() {
        let out = render_config(ClientShape::McpServers, false, "x", Some("/usr/bin/ai-mcp"));
        assert!(out.contains("\"command\": \"/usr/bin/ai-mcp\""));
    }

    #[test]
    fn agents_block_is_marker_wrapped() {
        assert!(AGENTS_MD.contains("<!-- wmaker-ai:begin -->"));
        assert!(AGENTS_MD.contains("<!-- wmaker-ai:end -->"));
    }

    /// The committed literal patch is a new-file diff of AGENTS.wmaker-ai.md, so
    /// its added lines must reconstruct the snippet exactly. Guards against
    /// editing the snippet without regenerating the patch (see integrations/README).
    #[test]
    fn patch_added_lines_reconstruct_the_agents_block() {
        let patch = include_str!("../../../integrations/agents/AGENTS.wmaker-ai.md.patch");
        let hunk_start = patch
            .lines()
            .position(|l| l.starts_with("@@"))
            .expect("patch has a hunk header");
        let reconstructed: String = patch
            .lines()
            .skip(hunk_start + 1)
            .map(|l| {
                let added = l
                    .strip_prefix('+')
                    .expect("new-file patch: every hunk body line is an addition");
                format!("{added}\n")
            })
            .collect();
        assert_eq!(reconstructed, AGENTS_MD);
    }
}
