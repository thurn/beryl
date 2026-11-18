//! Desktop notifications: `osascript` on macOS, a toast through PowerShell on Windows.

/// Posts a desktop notification, and prints it to standard error where the system has no way to
/// show one or the attempt fails. Never fails and does not wait for the user.
pub fn post(title: &str, text: &str) {
    if !show(title, text) {
        eprintln!("ignite: {title}: {text}");
    }
}

#[cfg(target_os = "macos")]
fn show(title: &str, text: &str) -> bool {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        quote(text),
        quote(title)
    );
    std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The title and text go in environment variables, so that no quoting is needed in the script.
#[cfg(windows)]
fn show(title: &str, text: &str) -> bool {
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
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("IGNITE_NOTE_TITLE", title)
        .env("IGNITE_NOTE_TEXT", text)
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn show(_title: &str, _text: &str) -> bool {
    false
}
