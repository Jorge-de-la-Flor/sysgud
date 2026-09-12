use crate::core::AgentRequest;

/// Prompt de sistema: le pide al modelo un JSON estricto, sin prosa ni
/// bloques de código, que calce con [`crate::core::AgentAction`].
pub const SYSTEM_PROMPT: &str = r#"You are SystemGuard, an autonomous runtime diagnostics agent.
You receive a snapshot of recent process logs and must return ONLY a JSON object
(no markdown fences, no prose, no explanation) with this exact shape:

{"action_type": "KILL" | "EXECUTE" | "NOTIFY", "command": string | null, "diagnosis": string}

Rules:
- Use "KILL" only when the target process must be terminated immediately.
- Use "EXECUTE" when a safe, non-destructive shell command can remediate the issue.
- Use "NOTIFY" when no automated action should be taken, only a diagnosis reported.
- "command" must be null unless action_type is "EXECUTE".
- "diagnosis" is a short, technical, human-readable explanation.
"#;

/// Arma el mensaje de usuario a partir del contexto y el extracto de logs.
pub fn build_user_message(request: &AgentRequest) -> String {
    format!(
        "system_context: {}\nlog_extract:\n{}",
        request.system_context,
        request.log_extract.join("\n")
    )
}

/// El modelo a veces envuelve el JSON en fences de markdown pese a que se
/// le pide que no lo haga; esta función los elimina de forma defensiva
/// antes de intentar parsear.
pub fn strip_code_fences(raw: &str) -> &str {
    raw.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quita_fences_de_markdown() {
        let raw = "```json\n{\"a\": 1}\n```";
        assert_eq!(strip_code_fences(raw), "{\"a\": 1}");
    }
}
