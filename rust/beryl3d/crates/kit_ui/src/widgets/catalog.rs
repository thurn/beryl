//! The named widgets used by stories and inspection.

use kit_frame::frame::Role;

// Define the names and paths once so every variant appears in the inventory.
macro_rules! widgets {
    ($($variant:ident = $word:literal in $module:ident;)*) => {
        /// A named toolkit widget.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Widget {
            $($variant,)*
        }

        impl Widget {
            /// Every named toolkit widget.
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The name used for this widget in stories.
            #[must_use]
            pub const fn word(self) -> &'static str {
                match self {
                    $(Self::$variant => $word,)*
                }
            }

            /// The workspace source file that implements this widget.
            #[must_use]
            pub const fn source(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("crates/kit_ui/src/widgets/", stringify!($module), ".rs"),)*
                }
            }
        }
    };
}

widgets! {
    Button = "button" in button;
    PassOn = "pass-on" in button;
    SmallButton = "small-button" in start_panel;
    CompactAction = "phone-action" in screen;
    Check = "check" in check;
    Checkbox = "checkbox" in start_panel;
    CheckSquare = "check-square" in start_panel;
    StartPanel = "start-panel" in start_panel;
    Field = "field" in field;
    IconButton = "icon-button" in icon_button;
    List = "list" in list;
    Menu = "menu" in menu;
    Slider = "slider" in slider;
    Toggle = "toggle" in toggle;
    Tooltip = "tooltip" in tooltip;
}

impl Widget {
    /// The role the widget produces in a story.
    #[must_use]
    pub const fn role(self) -> Role {
        match self {
            Self::Button
            | Self::PassOn
            | Self::IconButton
            | Self::SmallButton
            | Self::CompactAction => Role::Button,
            Self::Check | Self::CheckSquare | Self::StartPanel => Role::Region,
            Self::Field => Role::Field,
            Self::List | Self::Menu => Role::List,
            Self::Slider => Role::Slider,
            Self::Toggle | Self::Checkbox => Role::Toggle,
            Self::Tooltip => Role::Text,
        }
    }
}
