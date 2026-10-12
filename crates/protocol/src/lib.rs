//! Wire protocol shared by `peekaboo-hook` and the Peekaboo core.
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: u32 = 8 * 1024 * 1024;
/// Overrides the socket path / pipe name. Used by tests.
pub const SOCKET_ENV: &str = "PEEKABOO_SOCKET";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookMessage {
    pub v: u32,
    pub id: String,
    pub agent: String,
    pub event: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    pub payload: serde_json::Value,
    pub expects_reply: bool,
    /// The agent's `TERM_PROGRAM` (e.g. `Apple_Terminal`, `WarpTerminal`), when known.
    #[serde(default)]
    pub term: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookReply {
    pub v: u32,
    pub id: String,
    pub stdout: String,
    pub exit_code: i32,
}

impl HookReply {
    /// A reply that makes the agent behave as if no hook ran.
    pub fn passthrough(id: &str) -> Self {
        Self { v: PROTOCOL_VERSION, id: id.to_string(), stdout: String::new(), exit_code: 0 }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("frame too large: {0} bytes")]
    TooLarge(u32),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn encode_frame<T: Serialize>(msg: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(msg)?;
    let len = u32::try_from(body.len()).map_err(|_| FrameError::TooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

pub fn decode_len(header: [u8; 4]) -> Result<usize, FrameError> {
    let len = u32::from_be_bytes(header);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge(len));
    }
    Ok(len as usize)
}

pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> Result<(), FrameError> {
    w.write_all(&encode_frame(msg)?)?;
    w.flush()?;
    Ok(())
}

pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> Result<T, FrameError> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header)?;
    let mut body = vec![0u8; decode_len(header)?];
    r.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(unix)]
pub fn socket_path() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os(SOCKET_ENV) {
        return p.into();
    }
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Peekaboo").join("peekaboo.sock")
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Never fall back to shared /tmp: another user could bind the path first.
        dirs::runtime_dir()
            .map(|d| d.join("peekaboo.sock"))
            .or_else(|| dirs::data_local_dir().map(|d| d.join("peekaboo").join("peekaboo.sock")))
            .unwrap_or_else(|| std::path::PathBuf::from(".peekaboo.sock"))
    }
}

/// True only if `path` exists and belongs to the user running this process. The hook refuses
/// to talk to a socket someone else created.
#[cfg(unix)]
pub fn owned_by_current_user(path: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    std::fs::symlink_metadata(path).is_ok_and(|m| m.uid() == uid)
}

/// Per-user pipe name so users on one machine never share a pipe.
#[cfg(windows)]
pub fn pipe_name() -> String {
    std::env::var(SOCKET_ENV).unwrap_or_else(|_| {
        let user: String = std::env::var("USERNAME")
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        format!(r"\\.\pipe\peekaboo-{user}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn msg() -> HookMessage {
        HookMessage {
            v: PROTOCOL_VERSION, id: "abc".into(), agent: "claude".into(),
            event: "PermissionRequest".into(), cwd: Some("/tmp/x".into()), pid: Some(1),
            payload: serde_json::json!({"tool_name": "Bash"}), expects_reply: true, term: None,
        }
    }

    #[test]
    fn frame_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg()).unwrap();
        let back: HookMessage = read_frame(&mut Cursor::new(buf)).unwrap();
        assert_eq!(back, msg());
    }

    #[test]
    fn rejects_oversized_header() {
        let header = (MAX_FRAME_BYTES + 1).to_be_bytes();
        assert!(matches!(decode_len(header), Err(FrameError::TooLarge(_))));
    }

    #[test]
    fn truncated_frame_is_io_error() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg()).unwrap();
        buf.truncate(buf.len() - 3);
        let r: Result<HookMessage, _> = read_frame(&mut Cursor::new(buf));
        assert!(matches!(r, Err(FrameError::Io(_))));
    }

    #[test]
    fn passthrough_reply_is_empty_success() {
        let r = HookReply::passthrough("abc");
        assert_eq!((r.v, r.id.as_str(), r.stdout.as_str(), r.exit_code), (1, "abc", "", 0));
    }

    #[cfg(unix)]
    #[test]
    fn socket_path_respects_env() {
        std::env::set_var(SOCKET_ENV, "/tmp/pb-test.sock");
        assert_eq!(socket_path(), std::path::PathBuf::from("/tmp/pb-test.sock"));
        std::env::remove_var(SOCKET_ENV);
    }

    #[cfg(unix)]
    #[test]
    fn ownership_check() {
        let f = std::env::temp_dir().join(format!("pb-own-{}", std::process::id()));
        std::fs::write(&f, b"").unwrap();
        assert!(owned_by_current_user(&f));
        std::fs::remove_file(&f).unwrap();
        assert!(!owned_by_current_user(&f), "missing file is not trusted");
        assert!(!owned_by_current_user(std::path::Path::new("/")), "root-owned path is not trusted");
    }

    #[test]
    fn older_hooks_without_term_still_parse() {
        let old = r#"{"v":1,"id":"a","agent":"claude","event":"Stop","payload":{},"expects_reply":false}"#;
        let m: HookMessage = serde_json::from_str(old).unwrap();
        assert_eq!(m.term, None);
    }
}
