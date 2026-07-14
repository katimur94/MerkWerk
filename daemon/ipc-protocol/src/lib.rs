//! IPC protocol for MerkWerk daemon — Named Pipe communication.
//!
//! This crate is platform-neutral: it only defines the wire-format types and
//! their (de)serialization. It contains no Named Pipe / OS I/O code — that
//! lives in `merkwerk-daemon` behind `#[cfg(windows)]` (see ENTSCHEIDUNGEN.md
//! D1 and D6).
//!
//! The wire format is JSONL: one JSON object per line, terminated by a
//! single `\n`. Field/tag names below are a stable wire contract — do not
//! rename them without a protocol version bump.

use serde::{Deserialize, Serialize};

/// Well-known Windows Named Pipe path for the MerkWerk daemon IPC channel.
pub const PIPE_NAME: &str = r"\\.\pipe\merkwerk";

/// Requests sent from a client (e.g. the tray app) to the daemon.
///
/// Tagged as `{"cmd": "...", ...fields}` so the enum can grow new variants
/// with fields later without breaking existing clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    GetStatus,
    Pause,
    Resume,
    ReloadConfig,
    DistillNow { from_ms: i64, to_ms: i64 },
}

/// Responses sent from the daemon back to a client.
///
/// Tagged as `{"type": "...", ...fields}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Status {
        running: bool,
        paused: bool,
        events_captured: u64,
        snapshots_captured: u64,
        uptime_secs: u64,
    },
    Ok,
    Error {
        message: String,
    },
}

/// Errors that can occur while encoding or decoding IPC messages.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to (de)serialize IPC message: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result alias for IPC (de)serialization.
pub type Result<T> = std::result::Result<T, Error>;

/// Encode a [`Request`] as a single JSONL line (JSON + trailing `\n`).
pub fn encode_request(request: &Request) -> String {
    let mut line = serde_json::to_string(request).expect("Request serialization is infallible");
    line.push('\n');
    line
}

/// Decode a [`Request`] from a single line of JSON (leading/trailing
/// whitespace, including a trailing `\n`, is tolerated).
pub fn decode_request(line: &str) -> Result<Request> {
    Ok(serde_json::from_str(line.trim())?)
}

/// Encode a [`Response`] as a single JSONL line (JSON + trailing `\n`).
pub fn encode_response(response: &Response) -> String {
    let mut line = serde_json::to_string(response).expect("Response serialization is infallible");
    line.push('\n');
    line
}

/// Decode a [`Response`] from a single line of JSON (leading/trailing
/// whitespace, including a trailing `\n`, is tolerated).
pub fn decode_response(line: &str) -> Result<Response> {
    Ok(serde_json::from_str(line.trim())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_requests() -> Vec<Request> {
        vec![
            Request::GetStatus,
            Request::Pause,
            Request::Resume,
            Request::ReloadConfig,
            Request::DistillNow {
                from_ms: 1000,
                to_ms: 2000,
            },
        ]
    }

    fn all_responses() -> Vec<Response> {
        vec![
            Response::Status {
                running: true,
                paused: false,
                events_captured: 42,
                snapshots_captured: 7,
                uptime_secs: 3600,
            },
            Response::Ok,
            Response::Error {
                message: "something went wrong".to_string(),
            },
        ]
    }

    #[test]
    fn request_round_trip() {
        for request in all_requests() {
            let encoded = encode_request(&request);
            let decoded = decode_request(&encoded).expect("decode should succeed");
            assert_eq!(request, decoded);
        }
    }

    #[test]
    fn response_round_trip() {
        for response in all_responses() {
            let encoded = encode_response(&response);
            let decoded = decode_response(&encoded).expect("decode should succeed");
            assert_eq!(response, decoded);
        }
    }

    #[test]
    fn request_jsonl_ends_with_exactly_one_newline() {
        for request in all_requests() {
            let encoded = encode_request(&request);
            assert!(encoded.ends_with('\n'));
            assert!(!encoded.ends_with("\n\n"));
            assert_eq!(encoded.matches('\n').count(), 1);
        }
    }

    #[test]
    fn response_jsonl_ends_with_exactly_one_newline() {
        for response in all_responses() {
            let encoded = encode_response(&response);
            assert!(encoded.ends_with('\n'));
            assert!(!encoded.ends_with("\n\n"));
            assert_eq!(encoded.matches('\n').count(), 1);
        }
    }

    #[test]
    fn decode_request_known_wire_format() {
        assert_eq!(
            decode_request(r#"{"cmd":"get_status"}"#).unwrap(),
            Request::GetStatus
        );
        assert_eq!(
            decode_request(r#"{"cmd":"pause"}"#).unwrap(),
            Request::Pause
        );
        assert_eq!(
            decode_request(r#"{"cmd":"resume"}"#).unwrap(),
            Request::Resume
        );
        assert_eq!(
            decode_request(r#"{"cmd":"reload_config"}"#).unwrap(),
            Request::ReloadConfig
        );
        assert_eq!(
            decode_request(r#"{"cmd":"distill_now","from_ms":1000,"to_ms":2000}"#).unwrap(),
            Request::DistillNow {
                from_ms: 1000,
                to_ms: 2000
            }
        );
    }

    #[test]
    fn decode_response_known_wire_format() {
        assert_eq!(
            decode_response(
                r#"{"type":"status","running":true,"paused":false,"events_captured":42,"snapshots_captured":7,"uptime_secs":3600}"#
            )
            .unwrap(),
            Response::Status {
                running: true,
                paused: false,
                events_captured: 42,
                snapshots_captured: 7,
                uptime_secs: 3600,
            }
        );
        assert_eq!(decode_response(r#"{"type":"ok"}"#).unwrap(), Response::Ok);
        assert_eq!(
            decode_response(r#"{"type":"error","message":"oops"}"#).unwrap(),
            Response::Error {
                message: "oops".to_string(),
            }
        );
    }

    #[test]
    fn encode_request_known_wire_format() {
        assert_eq!(
            encode_request(&Request::GetStatus),
            "{\"cmd\":\"get_status\"}\n"
        );
    }

    #[test]
    fn encode_response_known_wire_format() {
        assert_eq!(encode_response(&Response::Ok), "{\"type\":\"ok\"}\n");
    }

    #[test]
    fn decode_request_rejects_garbage() {
        assert!(decode_request("not json").is_err());
        assert!(decode_request(r#"{"cmd":"unknown_cmd"}"#).is_err());
    }

    #[test]
    fn decode_response_rejects_garbage() {
        assert!(decode_response("not json").is_err());
        assert!(decode_response(r#"{"type":"unknown_type"}"#).is_err());
    }

    #[test]
    fn pipe_name_matches_decision_d1() {
        assert_eq!(PIPE_NAME, r"\\.\pipe\merkwerk");
    }
}
