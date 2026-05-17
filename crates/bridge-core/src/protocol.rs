use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCommand {
    pub seq: u32,
    pub command: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("empty line")]
    Empty,
    #[error("expected client command starting with C")]
    NotClientCommand,
    #[error("missing command separator")]
    MissingSeparator,
    #[error("invalid sequence number")]
    InvalidSequence,
    #[error("missing command body")]
    MissingBody,
}

pub fn parse_client_command(line: &str) -> Result<ClientCommand, ProtocolError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err(ProtocolError::Empty);
    }
    let rest = trimmed
        .strip_prefix('C')
        .ok_or(ProtocolError::NotClientCommand)?;
    let (seq, command) = rest
        .split_once('|')
        .ok_or(ProtocolError::MissingSeparator)?;
    let seq = seq
        .parse::<u32>()
        .map_err(|_| ProtocolError::InvalidSequence)?;
    let command = command.trim();
    if command.is_empty() {
        return Err(ProtocolError::MissingBody);
    }
    Ok(ClientCommand {
        seq,
        command: command.to_string(),
    })
}

pub fn response_line(seq: u32, code: u32, body: impl AsRef<str>) -> String {
    format!("R{}|{}|{}\n", seq, code, body.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_client_command() {
        let cmd = parse_client_command("C42|status\n").unwrap();
        assert_eq!(cmd.seq, 42);
        assert_eq!(cmd.command, "status");
    }

    #[test]
    fn rejects_bad_command() {
        assert_eq!(
            parse_client_command("R1|0|ok").unwrap_err(),
            ProtocolError::NotClientCommand
        );
    }

    #[test]
    fn formats_response() {
        assert_eq!(response_line(7, 0, "state=IDLE"), "R7|0|state=IDLE\n");
    }
}

