use std::io::{self, Read};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Builds a command for a copy of the test binary that stays alive until its stdin closes.
pub(crate) fn hold_stdin(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .args(["--exact", "test_process::read_stdin", "--ignored"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[test]
#[ignore = "run as a child process by other tests"]
fn read_stdin() {
    io::stdin().read_to_end(&mut Vec::new()).unwrap();
}

/// Prevents deletion of a file until the returned guard is dropped.
// Windows can deny delete sharing while a file is open.
#[cfg(windows)]
pub(crate) fn block_deletion(file: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt as _;

    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1) // FILE_SHARE_READ
        .open(file)
        .unwrap()
}

// POSIX requires write permission on the containing directory to unlink a file.
#[cfg(unix)]
pub(crate) fn block_deletion(file: &Path) -> RestorePermissions {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = file.parent().unwrap();
    let permissions = std::fs::metadata(dir).unwrap().permissions();
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    RestorePermissions(dir.to_owned(), permissions)
}

// Holds the original POSIX directory permissions until a deletion attempt finishes.
#[cfg(unix)]
pub(crate) struct RestorePermissions(std::path::PathBuf, std::fs::Permissions);

// Restores the original POSIX directory permissions.
#[cfg(unix)]
impl Drop for RestorePermissions {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, self.1.clone());
    }
}

/// A child connects when it reaches the gate and waits for the parent to release it.
pub(crate) struct ChildGate {
    listener: TcpListener,
    peer: OnceLock<TcpStream>,
}

impl ChildGate {
    pub(crate) fn new() -> Self {
        Self {
            listener: TcpListener::bind("127.0.0.1:0").unwrap(),
            peer: OnceLock::new(),
        }
    }

    pub(crate) fn address(&self) -> SocketAddr {
        self.listener.local_addr().unwrap()
    }

    pub(crate) fn wait_started(&self) {
        self.peer();
    }

    pub(crate) fn release(&self) {
        self.peer().shutdown(Shutdown::Write).unwrap();
    }

    fn peer(&self) -> &TcpStream {
        self.peer.get_or_init(|| self.listener.accept().unwrap().0)
    }

    pub(crate) fn wait(address: &str) {
        let mut peer = TcpStream::connect(address).unwrap();
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
    }
}
