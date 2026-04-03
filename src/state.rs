use crate::types::State;

/// Load state from the agent-supplied context, or fall back to reading
/// `polycopy-state.json` from the workspace on the very first call.
pub fn load(context: Option<&str>) -> Result<State, String> {
    if let Some(ctx) = context {
        let trimmed = ctx.trim();
        if !trimmed.is_empty() {
            return serde_json::from_str(trimmed)
                .map_err(|e| format!("parse context state: {e}"));
        }
    }

    // First-run bootstrap: read the file the user prepared.
    let raw = crate::near::agent::host::workspace_read("polycopy-state.json")
        .ok_or_else(|| {
            "No state in context and polycopy-state.json not found in workspace. \
             Copy polycopy-tool/polycopy-state.json to the workspace root and fill \
             in your credentials."
                .to_string()
        })?;

    serde_json::from_str(&raw).map_err(|e| format!("parse polycopy-state.json: {e}"))
}

