//! Redacción de credenciales y filtrado del entorno de procesos hijos.
use regex::Regex;
use tokio::process::Command;

fn sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "PRIVATE_KEY",
        "CREDENTIAL",
        "AUTHORIZATION",
    ]
    .iter()
    .any(|part| name.contains(part))
}

pub struct Redactor {
    secrets: Vec<String>,
    assignments: Regex,
    bearer: Regex,
    vendor_key: Regex,
}

impl Redactor {
    pub fn from_env() -> Self {
        Self::new(
            std::env::vars()
                .filter(|(name, _)| sensitive_name(name))
                .map(|(_, value)| value)
                .collect(),
        )
    }

    fn new(mut secrets: Vec<String>) -> Self {
        secrets.retain(|value| !value.is_empty());
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        Self {
            secrets,
            assignments: Regex::new(r#"(?i)((?:[\w-]*(?:token|secret|password|passwd|api[_-]?key|authorization|credential)[\w-]*)["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,;]+)"#).expect("constant regex"),
            bearer: Regex::new(r"(?i)\bBearer\s+[^\s,;]+").expect("constant regex"),
            vendor_key: Regex::new(r"\b(?:sk-|ghp_|github_pat_)[A-Za-z0-9_-]+").expect("constant regex"),
        }
    }

    pub fn redact(&self, line: &str) -> String {
        let mut value = line.to_owned();
        for secret in &self.secrets {
            value = value.replace(secret, "[REDACTED]");
        }
        value = self
            .assignments
            .replace_all(&value, "${1}[REDACTED]")
            .into_owned();
        value = self
            .bearer
            .replace_all(&value, "Bearer [REDACTED]")
            .into_owned();
        value = self
            .vendor_key
            .replace_all(&value, "[REDACTED]")
            .into_owned();
        value
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect()
    }
}

pub fn protect_environment(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        if sensitive_name(&name.to_string_lossy()) {
            command.env_remove(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_known_and_labelled_secrets() {
        let redactor = Redactor::new(vec!["known-sensitive-value".into()]);
        for input in [
            "ERROR token=abc123",
            "password: \"two words\"",
            "Bearer abc.def",
            "known-sensitive-value",
            "sk-ant-example123",
        ] {
            let clean = redactor.redact(input);
            assert!(clean.contains("[REDACTED]"), "{clean}");
            assert!(!clean.contains("abc123"));
            assert!(!clean.contains("two words"));
        }
        assert_eq!(redactor.redact("ERROR: disk full"), "ERROR: disk full");
        assert!(!redactor.redact("\x1b[2JERROR").contains('\x1b'));
    }
}
