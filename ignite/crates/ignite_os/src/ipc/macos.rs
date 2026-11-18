//! The macOS half of `ipc`: a Unix domain socket at the path `name`, inside a folder only the
//! user can enter, and itself readable and writable only by the user.
//!
//! The listener also holds an exclusive `flock` on `<name>.lock` for as long as it lives. That
//! lock, not whether the socket answers, says whether a daemon is listening: macOS creates a
//! socket and marks it close-on-exec in two steps, so a child started by another thread in
//! between keeps a dead daemon's socket answering. A file opened with `O_CLOEXEC`, as Rust
//! opens every file, never leaks that way.

use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

use super::{Ipc, Listener, Stream};
use crate::Native;

impl Stream for UnixStream {
    fn set_read_timeout(&self, limit: Option<Duration>) -> io::Result<()> {
        UnixStream::set_read_timeout(self, limit)
    }

    /// Peeks one byte without waiting: the end of the stream means the other end has closed.
    fn closed_probe(&self) -> Option<Box<dyn Fn() -> bool + Send + Sync>> {
        let own = self.try_clone().ok()?;
        Some(Box::new(move || {
            let mut byte = 0_u8;
            // SAFETY: `recv` writes at most one byte into `byte`, which lives through the
            // call, and reads the descriptor that `own` keeps open.
            let read = unsafe {
                libc::recv(
                    own.as_raw_fd(),
                    (&raw mut byte).cast(),
                    1,
                    libc::MSG_PEEK | libc::MSG_DONTWAIT,
                )
            };
            read == 0
        }))
    }
}

struct SocketListener {
    socket: UnixListener,
    /// Held, never read: the open file keeps the lock.
    _lock: File,
}

impl Listener for SocketListener {
    fn accept(&self) -> io::Result<Box<dyn Stream>> {
        let (stream, _) = self.socket.accept()?;
        Ok(Box::new(stream))
    }
}

impl Ipc for Native {
    fn listen(&self, name: &str) -> io::Result<Box<dyn Listener>> {
        let path = Path::new(name);
        if let Some(folder) = path
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
        {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(folder)?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(format!("{name}.lock"))?;
        // SAFETY: `flock` only reads the open descriptor and its flags.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("{name} is already being listened on"),
                ));
            }
            return Err(error);
        }
        // With the lock held, a socket file left here belongs to a daemon that stopped.
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let socket = UnixListener::bind(path)?;
        fs::set_permissions(path, Permissions::from_mode(0o600))?;
        Ok(Box::new(SocketListener {
            socket,
            _lock: lock,
        }))
    }

    fn connect(&self, name: &str) -> io::Result<Box<dyn Stream>> {
        Ok(Box::new(UnixStream::connect(name)?))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;
    use std::thread;

    use super::*;

    /// A fresh folder for one test. Socket paths are limited to about 100 bytes, so it is short.
    fn folder(test: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("ign-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn the_probe_sees_the_client_go_away() {
        let dir = folder("probe");
        let name = dir.join("d.sock");
        let name = name.to_str().unwrap().to_owned();
        let listener = Native.listen(&name).unwrap();
        let client = Native.connect(&name).unwrap();
        let served = listener.accept().unwrap();
        let probe = served.closed_probe().unwrap();
        assert!(!probe(), "the client is still connected");
        drop(client);
        // The close reaches the served end asynchronously; a blocking read returns at the end
        // of the stream, after which the probe must see it.
        let mut rest = Vec::new();
        let mut served = served;
        served.read_to_end(&mut rest).unwrap();
        assert!(probe(), "the client has closed its end");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stream_that_receives_nothing_times_out_its_read() {
        let dir = folder("timeout");
        let name = dir.join("d.sock");
        let name = name.to_str().unwrap().to_owned();
        let listener = Native.listen(&name).unwrap();
        let _client = Native.connect(&name).unwrap();
        let mut served = listener.accept().unwrap();
        served
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let mut byte = [0_u8; 1];
        let error = std::io::Read::read(&mut served, &mut byte).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_client_and_listener_exchange_a_line() {
        let dir = folder("line");
        let name = dir.join("sub").join("d.sock");
        let name = name.to_str().unwrap().to_owned();
        let listener = Native.listen(&name).unwrap();

        assert_eq!(fs::metadata(&name).unwrap().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(dir.join("sub")).unwrap().mode() & 0o777, 0o700);

        let server = thread::spawn(move || {
            let mut stream = BufReader::new(listener.accept().unwrap());
            let mut line = String::new();
            stream.read_line(&mut line).unwrap();
            stream
                .get_mut()
                .write_all(format!("echo {line}").as_bytes())
                .unwrap();
        });
        let mut client = BufReader::new(Native.connect(&name).unwrap());
        client
            .get_mut()
            .write_all(b"{\"verb\":\"status\"}\n")
            .unwrap();
        let mut reply = String::new();
        client.read_line(&mut reply).unwrap();
        server.join().unwrap();
        assert_eq!(reply, "echo {\"verb\":\"status\"}\n");
        fs::remove_dir_all(dir).unwrap();
    }

    /// The variable that tells [`listen_and_stop`] where to listen.
    const STOPPED_SOCKET: &str = "IGNITE_TEST_STOPPED_SOCKET";

    /// Run by `a_stale_socket_is_replaced_and_a_live_one_is_not` as a daemon that stops
    /// without removing its socket. It runs in a process of its own because a listener
    /// dropped in the test process can stay locked for a moment: a child that another test
    /// starts at that time briefly holds copies of every descriptor, close-on-exec ones too.
    #[test]
    #[ignore = "run by a_stale_socket_is_replaced_and_a_live_one_is_not"]
    fn listen_and_stop() {
        if let Some(name) = std::env::var_os(STOPPED_SOCKET) {
            let _listener = Native.listen(name.to_str().unwrap()).unwrap();
            std::process::exit(0);
        }
    }

    #[test]
    fn a_stale_socket_is_replaced_and_a_live_one_is_not() {
        let dir = folder("stale");
        let name = dir.join("d.sock");
        let name = name.to_str().unwrap().to_owned();
        let stopped = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "ipc::macos::tests::listen_and_stop", "--ignored"])
            .env(STOPPED_SOCKET, &name)
            .output()
            .unwrap();
        assert!(
            stopped.status.success(),
            "{}",
            String::from_utf8_lossy(&stopped.stdout)
        );
        assert!(Path::new(&name).exists());

        let _listener = Native.listen(&name).unwrap();
        Native.connect(&name).unwrap();
        let error = Native.listen(&name).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        fs::remove_dir_all(dir).unwrap();
    }
}
