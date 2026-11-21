//! Shared `Content-Length` framing for LSP and DAP.
//! Important: protocol message semantics differ: LSP is JSON-RPC, DAP is not.
const MAX_MESSAGE: usize = 32 * 1024 * 1024;
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    InvalidHeader,
    InvalidLength,
    TooLarge,
}
/// Return the complete payload and bytes consumed, or None if the frame is incomplete.
pub fn decode(input: &[u8]) -> Result<Option<(Vec<u8>, usize)>, FrameError> {
    let end = match input.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(i) => i,
        None => {
            if input.len() > 16 * 1024 {
                return Err(FrameError::InvalidHeader);
            };
            return Ok(None);
        }
    };
    if end > 16 * 1024 {
        return Err(FrameError::InvalidHeader);
    }
    let header = std::str::from_utf8(&input[..end]).map_err(|_| FrameError::InvalidHeader)?;
    let mut length = None;
    for line in header.split("\r\n") {
        if let Some((key, value)) = line.split_once(':') {
            if key.trim().eq_ignore_ascii_case("content-length") {
                if length.is_some() {
                    return Err(FrameError::InvalidHeader);
                }
                let value = value.trim();
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(FrameError::InvalidLength);
                }
                length = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| FrameError::InvalidLength)?,
                );
            }
        } else {
            return Err(FrameError::InvalidHeader);
        }
    }
    let len = length.ok_or(FrameError::InvalidHeader)?;
    if len > MAX_MESSAGE {
        return Err(FrameError::TooLarge);
    }
    let start = end + 4;
    let finish = start.checked_add(len).ok_or(FrameError::TooLarge)?;
    if input.len() < finish {
        return Ok(None);
    }
    Ok(Some((input[start..finish].to_vec(), finish)))
}
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() > MAX_MESSAGE {
        return Err(FrameError::TooLarge);
    }
    let mut result = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
    result.extend_from_slice(payload);
    Ok(result)
}

/// Split a byte stream into complete frames and return the consumed prefix length.
pub fn drain(input: &[u8]) -> Result<(Vec<Vec<u8>>, usize), FrameError> {
    let mut frames = Vec::new();
    let mut offset = 0;
    while let Some((payload, consumed)) = decode(&input[offset..])? {
        if consumed == 0 {
            break;
        }
        frames.push(payload);
        offset += consumed;
    }
    Ok((frames, offset))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        let bytes = encode(br#"{"jsonrpc":"2.0"}"#).unwrap();
        let (out, consumed) = decode(&bytes).unwrap().unwrap();
        assert_eq!(out, br#"{"jsonrpc":"2.0"}"#);
        assert_eq!(consumed, bytes.len());
    }
    #[test]
    fn partial() {
        assert_eq!(decode(b"Content-Length: 12\r\n\r\nhello").unwrap(), None);
    }
    #[test]
    fn rejects_invalid() {
        assert_eq!(
            decode(b"Content-Length: xx\r\n\r\n"),
            Err(FrameError::InvalidLength)
        );
    }
    #[test]
    fn drains_two_frames_and_keeps_a_partial_tail() {
        let one = encode(b"{\"a\":1}").unwrap();
        let two = encode(b"{\"b\":2}").unwrap();
        let mut bytes = one.clone();
        bytes.extend_from_slice(&two);
        bytes.extend_from_slice(&two[..4]);
        let (frames, consumed) = drain(&bytes).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], b"{\"a\":1}");
        assert_eq!(consumed, one.len() + two.len());
    }
}
