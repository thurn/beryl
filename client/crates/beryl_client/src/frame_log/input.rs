//! The frame log's input lines: each input a host handed the client that the log keeps, with
//! when it came.

use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Modifiers, TextChange};
use serde::{Deserialize, Serialize};

use app_client::Input;

/// An input line as written: the input, and how long before it the last frame was drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inputted {
    #[serde(flatten)]
    pub input: InputLine,
    /// Milliseconds since the frame before this input was drawn, or none when no frame had
    /// been. A phone smoke test reads it on the last input to prove the host stopped asking for
    /// frames while nothing moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_frame_ms: Option<f64>,
    /// Milliseconds since the log was made, as the host opened: the clock of a late line's
    /// `at_ms`, give or take the time the host took to open. Logs before this field have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<f64>,
    /// When the operating system stamped the event, on `at_ms`'s clock, where the host knows it
    /// (a pointer event on macOS). A gap in `at_ms` with no gap here means the app took the
    /// events late; a gap here too means the system sent none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_ms: Option<f64>,
}

/// An input a host handed the client, as a line `{"input": {"<kind>": {...}}}`. The phone
/// hosts' smoke tests read these to prove their events arrived. Enumerated values are written
/// as their Rust names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputLine {
    /// Runtime data was read and validated after an edit.
    DataReloaded {},
    /// Runtime data failed to load. The client keeps the last valid tuning.
    DataReloadFailed {
        error: String,
    },
    Pointer {
        id: u8,
        kind: String,
        phase: String,
        /// Logical points from the surface's top left.
        x: f32,
        y: f32,
        button: String,
        wheel_x: f32,
        wheel_y: f32,
        modifiers: Vec<String>,
    },
    Key {
        code: String,
        phase: String,
        modifiers: Vec<String>,
    },
    /// Text typed into the edited field, or its new contents, as its length in characters:
    /// the text itself may be private.
    Text {
        length: usize,
    },
    /// The surface's size and safe-area insets in logical points, and its pixels per point.
    Surface {
        width: f32,
        height: f32,
        scale: f32,
        #[serde(default)]
        top: f32,
        #[serde(default)]
        right: f32,
        #[serde(default)]
        bottom: f32,
        #[serde(default)]
        left: f32,
    },
    Lifecycle {
        event: String,
    },
}

impl InputLine {
    /// The line for `input`, or `None` for an input the log does not record (views, assets,
    /// replies).
    #[must_use]
    pub fn of(input: &Input) -> Option<Self> {
        Some(match input {
            Input::Data(_) => Self::DataReloaded {},
            Input::DataError(error) => Self::DataReloadFailed {
                error: error.clone(),
            },
            Input::Device(DeviceInput::Pointer(pointer)) => Self::Pointer {
                id: pointer.id,
                kind: format!("{:?}", pointer.kind),
                phase: format!("{:?}", pointer.phase),
                x: pointer.position.x,
                y: pointer.position.y,
                button: format!("{:?}", pointer.button),
                wheel_x: pointer.wheel.x,
                wheel_y: pointer.wheel.y,
                modifiers: modifier_names(pointer.modifiers),
            },
            Input::Device(DeviceInput::Key(key)) => Self::Key {
                code: format!("{:?}", key.code),
                phase: format!("{:?}", key.phase),
                modifiers: modifier_names(key.modifiers),
            },
            Input::Device(DeviceInput::Text(
                TextChange::Replace(text) | TextChange::Insert(text) | TextChange::Preedit(text),
            )) => Self::Text {
                length: text.chars().count(),
            },
            Input::Device(DeviceInput::Surface {
                size,
                scale,
                safe_area,
            }) => Self::Surface {
                width: size.width,
                height: size.height,
                scale: *scale,
                top: safe_area.top,
                right: safe_area.right,
                bottom: safe_area.bottom,
                left: safe_area.left,
            },
            Input::Device(DeviceInput::Lifecycle(lifecycle)) => Self::Lifecycle {
                event: format!("{lifecycle:?}"),
            },
            _ => return None,
        })
    }
}

/// The names of the modifier keys held.
fn modifier_names(modifiers: Modifiers) -> Vec<String> {
    [
        (modifiers.shift, "Shift"),
        (modifiers.control, "Control"),
        (modifiers.alt, "Alt"),
        (modifiers.command, "Command"),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .map(|(_, name)| name.to_owned())
    .collect()
}
