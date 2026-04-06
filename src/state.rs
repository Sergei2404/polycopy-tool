use crate::types::Context;

/// Load context from the agent-supplied string, or return a fresh default.
pub fn load(context: Option<&str>) -> Result<Context, String> {
    match context {
        Some(ctx) if !ctx.trim().is_empty() => serde_json::from_str(ctx.trim())
            .map_err(|e| format!("parse context: {e}")),
        _ => Ok(Context::default()),
    }
}
