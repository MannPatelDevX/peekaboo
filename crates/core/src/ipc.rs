//! Accepts `peekaboo-hook` connections and routes them through `Core`.
use crate::core::{Core, Handled};
use peekaboo_protocol::{decode_len, encode_frame, FrameError, HookMessage};
use serde::{de::DeserializeOwned, Serialize};
#[cfg(unix)]
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) async fn read_frame_async<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<T, FrameError> {
    let mut header = [0u8; 4];
    r.read_exact(&mut header).await?;
    let mut body = vec![0u8; decode_len(header)?];
    r.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

async fn write_frame_async<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<(), FrameError> {
    w.write_all(&encode_frame(msg)?).await?;
    w.flush().await?;
    Ok(())
}

async fn handle_conn<S: AsyncRead + AsyncWrite + Unpin>(core: Arc<Core>, mut stream: S) -> Result<(), FrameError> {
    let msg: HookMessage = match read_frame_async(&mut stream).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("peekaboo: dropped bad hook frame: {e}");
            return Ok(());
        }
    };
    let expects_reply = msg.expects_reply;
    match core.handle(msg) {
        Handled::Reply(reply) => {
            if expects_reply {
                write_frame_async(&mut stream, &reply).await?;
            }
        }
        Handled::Wait { msg, rx } => {
            let mut probe = [0u8; 1];
            // A read completing means the hook hung up (it never sends a second frame).
            let decision = tokio::select! {
                d = rx => d.ok(),
                _ = stream.read(&mut probe) => None,
            };
            match decision {
                Some(d) => write_frame_async(&mut stream, &core.render_reply(&msg, &d)).await?,
                None => core.cancel(&msg.id),
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
pub async fn serve_unix(core: Arc<Core>, path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        if !dir.exists() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    // A socket that still answers belongs to another running Peekaboo; only clear dead ones.
    if tokio::net::UnixStream::connect(path).await.is_ok() {
        return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "another Peekaboo is already running"));
    }
    let _ = std::fs::remove_file(path);
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let core = core.clone();
                tokio::spawn(async move {
                    let _ = handle_conn(core, stream).await;
                });
            }
            // Transient (e.g. out of file descriptors): keep serving.
            Err(e) => {
                eprintln!("peekaboo: accept failed: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

#[cfg(windows)]
pub async fn serve_pipe(core: Arc<Core>, name: &str) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let mut server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(name)?;
    loop {
        // A client that connected and left before we got here makes connect() fail; just recycle the instance.
        let connected = server.connect().await;
        let instance = std::mem::replace(&mut server, ServerOptions::new().reject_remote_clients(true).create(name)?);
        match connected {
            Ok(()) => {
                let core = core.clone();
                tokio::spawn(async move {
                    let _ = handle_conn(core, instance).await;
                });
            }
            Err(e) => eprintln!("peekaboo: pipe connect failed: {e}"),
        }
    }
}

/// Serves on the platform's default socket / pipe.
pub async fn serve(core: Arc<Core>) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        serve_unix(core, &peekaboo_protocol::socket_path()).await
    }
    #[cfg(windows)]
    {
        serve_pipe(core, &peekaboo_protocol::pipe_name()).await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use peekaboo_protocol::{encode_frame, HookReply};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    async fn start() -> (tempfile::TempDir, std::path::PathBuf, Arc<Core>) {
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join("pb.sock");
        let core = Arc::new(Core::new(d.path().join("settings.json"), Arc::new(|| {})));
        let (c2, s2) = (core.clone(), sock.clone());
        tokio::spawn(async move { serve_unix(c2, &s2).await.unwrap() });
        for _ in 0..50 {
            if UnixStream::connect(&sock).await.is_ok() { break }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        (d, sock, core)
    }

    fn stop_msg(id: &str) -> HookMessage {
        HookMessage { v: 1, id: id.into(), agent: "claude".into(), event: "Stop".into(), cwd: None, pid: None,
            payload: json!({"session_id": "s"}), expects_reply: true, term: None }
    }

    async fn round_trip(sock: &Path, m: &HookMessage) -> HookReply {
        let mut s = UnixStream::connect(sock).await.unwrap();
        s.write_all(&encode_frame(m).unwrap()).await.unwrap();
        read_frame_async(&mut s).await.unwrap()
    }

    #[tokio::test]
    async fn socket_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, sock, _) = start().await;
        assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[tokio::test]
    async fn replies_passthrough_for_notification_needing_reply() {
        let (_d, sock, _) = start().await;
        let r = round_trip(&sock, &stop_msg("x")).await;
        assert_eq!((r.id.as_str(), r.stdout.as_str()), ("x", ""));
    }

    #[tokio::test]
    async fn garbage_frame_does_not_kill_server() {
        let (_d, sock, _) = start().await;
        let mut s = UnixStream::connect(&sock).await.unwrap();
        s.write_all(&[0, 0, 0, 5, b'n', b'o', b'p', b'e', b'!']).await.unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(s.read(&mut buf).await.unwrap(), 0); // server closed this connection
        let r = round_trip(&sock, &stop_msg("y")).await;
        assert_eq!(r.id, "y");
    }

    #[tokio::test]
    async fn serve_replaces_stale_socket_file() {
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join("pb.sock");
        std::fs::write(&sock, b"stale").unwrap();
        let core = Arc::new(Core::new(d.path().join("settings.json"), Arc::new(|| {})));
        let s2 = sock.clone();
        tokio::spawn(async move { serve_unix(core, &s2).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(round_trip(&sock, &stop_msg("z")).await.id, "z");
    }

    #[tokio::test]
    async fn second_instance_does_not_steal_live_socket() {
        let (d, sock, _) = start().await;
        let other = Arc::new(Core::new(d.path().join("other.json"), Arc::new(|| {})));
        let err = serve_unix(other, &sock).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert_eq!(round_trip(&sock, &stop_msg("still-first")).await.id, "still-first");
    }
}
