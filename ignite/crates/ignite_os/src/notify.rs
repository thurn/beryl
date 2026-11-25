//! Desktop notifications: `osascript` on macOS, a toast through PowerShell on Windows.
//!
//! The helper process runs on its own and nobody waits for it. On 2026-11-25 a Windows machine's
//! trees drive stopped answering and PowerShell hung while it started; `ignite monitor` waited
//! for the toast's PowerShell, never finished its run, and Task Scheduler starts no new run of
//! the monitor while one is still running, so the machine got no restart or repair for hours.

use std::process::Child;
#[cfg(any(target_os = "macos", windows))]
use std::process::{Command, Stdio};

/// Posts a desktop notification, and prints it to standard error where the system has no way to
/// show one or the helper cannot be started. Never fails, and returns as soon as the helper has
/// started: it waits neither for the user nor for the helper.
pub fn post(title: &str, text: &str) {
    match show(title, text) {
        // A thread reaps the helper, so a long-running process leaves no zombie behind; in a
        // short-lived one the thread ends with the process.
        Some(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        None => eprintln!("ignite: {title}: {text}"),
    }
}

#[cfg(target_os = "macos")]
fn show(title: &str, text: &str) -> Option<Child> {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        quote(text),
        quote(title)
    );
    Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// The title and text go in environment variables, so that no quoting is needed in the script.
#[cfg(windows)]
fn show(title: &str, text: &str) -> Option<Child> {
    const SCRIPT: &str = "\
        $null = [Windows.UI.Notifications.ToastNotificationManager, \
            Windows.UI.Notifications, ContentType = WindowsRuntime]; \
        $xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent(\
            [Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
        $lines = $xml.GetElementsByTagName('text'); \
        $null = $lines.Item(0).AppendChild($xml.CreateTextNode($env:IGNITE_NOTE_TITLE)); \
        $null = $lines.Item(1).AppendChild($xml.CreateTextNode($env:IGNITE_NOTE_TEXT)); \
        $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); \
        [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Ignite')\
            .Show($toast)";
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("IGNITE_NOTE_TITLE", title)
        .env("IGNITE_NOTE_TEXT", text)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn show(_title: &str, _text: &str) -> Option<Child> {
    None
}
