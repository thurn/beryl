use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use beryl_link::LinkError;
use iroh::{EndpointId, SecretKey};

use crate::io_error;

/// The endpoint key stored in the app's data directory.
#[derive(Clone)]
pub struct Identity(pub(crate) SecretKey);

impl Identity {
    /// Load the existing key or publish a newly generated key at `path`.
    /// The caller supplies an existing parent directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LinkError> {
        let path = path.as_ref();
        match File::open(path) {
            Ok(file) => return Self::read(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }

        let directory = path.parent().filter(|path| !path.as_os_str().is_empty());
        let mut pending = tempfile::NamedTempFile::new_in(directory.unwrap_or(Path::new(".")))
            .map_err(io_error)?;
        // A named temporary file starts with mode 0600 on Unix. On Windows,
        // remove inherited permissions before putting any key bytes on disk.
        #[cfg(windows)]
        restrict_to_current_user(pending.path())?;
        let key = SecretKey::generate();
        pending.write_all(&key.to_bytes()).map_err(io_error)?;
        pending.as_file().sync_all().map_err(io_error)?;
        match pending.persist_noclobber(path) {
            Ok(_) => Ok(Self(key)),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                Self::read(File::open(path).map_err(io_error)?)
            }
            Err(error) => Err(io_error(error.error)),
        }
    }

    pub fn endpoint_id(&self) -> EndpointId {
        self.0.public()
    }

    fn read(file: File) -> Result<Self, LinkError> {
        let mut bytes = Vec::with_capacity(33);
        file.take(33).read_to_end(&mut bytes).map_err(io_error)?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
            LinkError::Io("The Beryl identity file must contain exactly 32 bytes.".into())
        })?;
        Ok(Self(SecretKey::from_bytes(&bytes)))
    }
}

#[cfg(windows)]
fn restrict_to_current_user(path: &Path) -> Result<(), LinkError> {
    use std::{env, os::windows::process::CommandExt, path::PathBuf, process::Command};

    // Use the SID rather than an environment variable or a localized account name.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let system = env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| LinkError::Io("Could not locate the Windows system directory.".into()))?
        .join("System32");
    let user = Command::new(system.join("whoami.exe"))
        .creation_flags(CREATE_NO_WINDOW)
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .map_err(io_error)?;
    let output = String::from_utf8_lossy(&user.stdout);
    let sid = output
        .trim()
        .rsplit_once(',')
        .map(|(_, sid)| sid.trim_matches('"'))
        .filter(|sid| {
            sid.strip_prefix("S-1-").is_some_and(|digits| {
                !digits.is_empty()
                    && digits
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || byte == b'-')
            })
        })
        .filter(|_| user.status.success())
        .ok_or_else(|| LinkError::Io("Could not identify the owner of the Beryl key.".into()))?;
    let permission = format!("*{sid}:(F)");
    let status = Command::new(system.join("icacls.exe"))
        .creation_flags(CREATE_NO_WINDOW)
        .arg(path)
        .args(["/grant:r", &permission, "/inheritancelevel:r"])
        .output()
        .map_err(io_error)?
        .status;
    if !status.success() {
        return Err(LinkError::Io(
            "Could not make the Beryl key private.".into(),
        ));
    }
    Ok(())
}
