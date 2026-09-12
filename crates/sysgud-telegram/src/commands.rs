use uuid::Uuid;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Status,
    Detail(Uuid),
    Approve(Uuid),
    Reject(Uuid),
    Help,
    Unknown,
}

/// Adapted from origin/telegram. Decisions require a specific incident ID.
pub(crate) fn parse(text: &str) -> Command {
    let words: Vec<_> = text.split_whitespace().collect();
    let Some(first) = words.first() else {
        return Command::Unknown;
    };
    let name = first.split('@').next().unwrap_or(first);
    if words.len() == 1 {
        return match name {
            "/status" | "/incidents" => Command::Status,
            "/start" | "/help" => Command::Help,
            _ => Command::Unknown,
        };
    }
    if words.len() != 2 {
        return Command::Unknown;
    }
    let Ok(id) = words[1].parse() else {
        return Command::Unknown;
    };
    match name {
        "/incident" => Command::Detail(id),
        "/approve" => Command::Approve(id),
        "/reject" => Command::Reject(id),
        _ => Command::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_target_required_and_suffix_does_not_swallow_arguments() {
        let id = Uuid::new_v4();
        assert_eq!(
            parse(&format!("/approve@sysgud {id}")),
            Command::Approve(id)
        );
        for value in [
            "/approve",
            "/approve@sysgud extra",
            "/status arbitrary",
            "/approve@sysgud extra hidden",
            "/reject ../../file",
        ] {
            assert_eq!(parse(value), Command::Unknown);
        }
        assert_eq!(parse("/status@sysgud"), Command::Status);
    }
}
