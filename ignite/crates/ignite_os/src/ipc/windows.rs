//! The Windows half of `ipc`: a named pipe that only the current user may open, and only from
//! this machine.
//!
//! `name` is the socket path the macOS half would use, such as `...\ignite\daemon.sock`. A
//! pipe lives in its own namespace rather than the file system, so the path becomes the pipe
//! `\\.\pipe\ignite-<path with / for \>`. A name that is already a pipe name is used as it is.
//!
//! The listener creates the first instance of the pipe with `FILE_FLAG_FIRST_PIPE_INSTANCE`,
//! which fails while another listener holds the name. A pipe vanishes with its last handle, so
//! no stale endpoint is ever left behind, unlike a Unix socket file.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::ptr;
use std::sync::{Mutex, OnceLock, PoisonError};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
    GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::{Ipc, Listener, Stream};
use crate::Native;

/// The size of each direction's buffer in the pipe.
const BUFFER: u32 = 64 * 1024;
/// How long a client waits for a free instance of a busy pipe, in milliseconds.
const BUSY_WAIT_MS: u32 = 5_000;

/// One end of a connected pipe.
struct PipeStream {
    file: File,
    /// True for the listener's end, which waits for the client to read everything written
    /// before it closes.
    server: bool,
}

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Stream for PipeStream {}

impl Drop for PipeStream {
    fn drop(&mut self) {
        if self.server {
            // SAFETY: the handle is open until `file` drops after this.
            unsafe { FlushFileBuffers(self.file.as_raw_handle()) };
        }
    }
}

struct PipeListener {
    /// The pipe's name as a NUL-terminated wide string.
    name: Vec<u16>,
    /// The instance the next client will connect to. `None` only after creating one failed;
    /// the next `accept` tries again.
    pending: Mutex<Option<File>>,
}

impl Listener for PipeListener {
    fn accept(&self) -> io::Result<Box<dyn Stream>> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let instance = match pending.take() {
            Some(instance) => instance,
            None => create_instance(&self.name, false)?,
        };
        let connected = connect_client(&instance);
        if let Err(error) = connected {
            *pending = Some(instance);
            return Err(error);
        }
        // The next instance is created before this one is handed out, so a client arriving
        // meanwhile waits for it rather than finding no pipe.
        *pending = create_instance(&self.name, false).ok();
        Ok(Box::new(PipeStream {
            file: instance,
            server: true,
        }))
    }
}

/// Waits until a client connects to `instance`.
fn connect_client(instance: &File) -> io::Result<()> {
    let handle = instance.as_raw_handle();
    loop {
        // SAFETY: the handle is an open pipe instance in blocking mode, so no overlapped record
        // is needed.
        if unsafe { ConnectNamedPipe(handle, ptr::null_mut()) } != 0 {
            return Ok(());
        }
        // SAFETY: plain call that reads the thread's last error.
        match unsafe { GetLastError() } {
            // A client connected between creating the instance and this call.
            ERROR_PIPE_CONNECTED => return Ok(()),
            // A client connected and closed already: reset the instance and wait again.
            ERROR_NO_DATA => {
                // SAFETY: as above.
                unsafe { DisconnectNamedPipe(handle) };
            }
            _ => return Err(io::Error::last_os_error()),
        }
    }
}

impl Ipc for Native {
    fn listen(&self, name: &str) -> io::Result<Box<dyn Listener>> {
        let name = wide(&pipe_name(name));
        let first = create_instance(&name, true)?;
        Ok(Box::new(PipeListener {
            name,
            pending: Mutex::new(Some(first)),
        }))
    }

    fn connect(&self, name: &str) -> io::Result<Box<dyn Stream>> {
        let name = wide(&pipe_name(name));
        loop {
            // SAFETY: `name` is a NUL-terminated wide string. The quality-of-service flags let
            // the listener identify the client but not act as it.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(Box::new(PipeStream {
                    // SAFETY: the handle is open and owned by nothing else.
                    file: unsafe { File::from_raw_handle(handle) },
                    server: false,
                }));
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
                return Err(error);
            }
            // Every instance is connected: wait for the listener to create another.
            // SAFETY: `name` is a NUL-terminated wide string.
            if unsafe { WaitNamedPipeW(name.as_ptr(), BUSY_WAIT_MS) } == 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }
}

/// The pipe name for `name`.
fn pipe_name(name: &str) -> String {
    const PREFIX: &str = r"\\.\pipe\";
    if name.starts_with(PREFIX) {
        name.to_owned()
    } else {
        format!("{PREFIX}ignite-{}", name.replace('\\', "/"))
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Creates one instance of the pipe `name`, which only the current user may open. With
/// `first`, fails with `AddrInUse` if the pipe already exists.
fn create_instance(name: &[u16], first: bool) -> io::Result<File> {
    let descriptor = Descriptor::for_current_user()?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let open_mode = PIPE_ACCESS_DUPLEX
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    // SAFETY: `name` is a NUL-terminated wide string, and `attributes` and the descriptor it
    // points to outlive the call.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER,
            BUFFER,
            0,
            &raw const attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        if first && error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!(
                    "{} is already being listened on",
                    String::from_utf16_lossy(name)
                ),
            ));
        }
        return Err(error);
    }
    // SAFETY: the handle is open and owned by nothing else.
    Ok(unsafe { File::from_raw_handle(handle) })
}

/// A security descriptor allocated by Windows, freed when dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// A descriptor whose access list grants the current user everything and nobody else
    /// anything.
    fn for_current_user() -> io::Result<Descriptor> {
        static SDDL: OnceLock<Vec<u16>> = OnceLock::new();
        let sddl = if let Some(sddl) = SDDL.get() {
            sddl
        } else {
            let sddl = wide(&format!("D:P(A;;GA;;;{})", current_user_sid()?));
            SDDL.get_or_init(|| sddl)
        };
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is a NUL-terminated wide string; the descriptor is freed on drop.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Descriptor(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by Windows with `LocalAlloc`.
        unsafe { LocalFree(self.0) };
    }
}

/// The current user's security identifier as text, such as `S-1-5-21-...`.
fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the current process's pseudo-handle needs no closing; the token is closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let sid = token_user_sid(token);
    // SAFETY: the token handle is open and owned here.
    unsafe { CloseHandle(token) };
    sid
}

fn token_user_sid(token: HANDLE) -> io::Result<String> {
    // A `TOKEN_USER` is followed by the identifier it points to, so the buffer is larger than
    // the record. Pointer-sized words keep it aligned for the record.
    let mut buffer = vec![0usize; 128];
    let mut length = 0;
    // SAFETY: `buffer` is writable and as long as the length passed.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * size_of::<usize>()) as u32,
            &raw mut length,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call filled the buffer, which starts with the record.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text: *mut u16 = ptr::null_mut();
    // SAFETY: the identifier is valid while `buffer` lives; the text is freed below.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call returned a NUL-terminated wide string.
    let sid = unsafe {
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
    };
    // SAFETY: the text was allocated by Windows with `LocalAlloc`.
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::thread;

    use super::*;

    /// A pipe name of this test process's own, so tests running at once do not meet.
    fn name(test: &str) -> String {
        format!(r"C:\ignite-test\{}\{test}.sock", std::process::id())
    }

    #[test]
    fn paths_become_pipe_names_and_pipe_names_stay() {
        assert_eq!(
            pipe_name(r"C:\Users\me\.config\ignite\daemon.sock"),
            r"\\.\pipe\ignite-C:/Users/me/.config/ignite/daemon.sock"
        );
        assert_eq!(pipe_name(r"\\.\pipe\other"), r"\\.\pipe\other");
    }

    #[test]
    fn clients_and_the_listener_exchange_lines() {
        let name = name("line");
        let listener = Native.listen(&name).unwrap();
        let server = thread::spawn(move || {
            for _ in 0..2 {
                let mut stream = BufReader::new(listener.accept().unwrap());
                let mut line = String::new();
                stream.read_line(&mut line).unwrap();
                stream
                    .get_mut()
                    .write_all(format!("echo {line}").as_bytes())
                    .unwrap();
            }
        });
        for request in ["{\"verb\":\"status\"}\n", "{\"verb\":\"doctor\"}\n"] {
            let mut client = BufReader::new(Native.connect(&name).unwrap());
            client.get_mut().write_all(request.as_bytes()).unwrap();
            let mut reply = String::new();
            client.read_line(&mut reply).unwrap();
            assert_eq!(reply, format!("echo {request}"));
        }
        server.join().unwrap();
    }

    #[test]
    fn a_second_listener_is_refused_and_a_missing_one_cannot_be_reached() {
        let name = name("busy");
        let listener = Native.listen(&name).unwrap();
        let error = Native.listen(&name).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        drop(listener);
        // The pipe vanished with its listener, so the name is free again.
        drop(Native.listen(&name).unwrap());
        assert!(Native.connect(&self::name("missing")).is_err());
    }
}
