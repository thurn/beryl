//! The manual table’s context menu and its phone pages.

mod view;
pub use view::{Inputs, ViewKey, sheet_face};

use crate::manual::{
    Target,
    menus::{Act, OpenMenu},
};
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use kit_ui::widgets::MenuPage;
use std::convert::Infallible;

pub type State = Option<OpenMenu>;

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Open(OpenMenu),
    Choose { target: Target, act: Act },
    Page(Option<MenuPage>),
    Close,
}

pub struct ContextMenu;

impl Feature for ContextMenu {
    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Infallible;
    type Request = (Act, Target);

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Self::Command, Self::Request>,
    ) -> bool {
        if let Message::Open(menu) = message {
            *state = Some(menu.clone());
            return true;
        }
        let Some(open) = state.as_mut() else {
            return false;
        };
        match message {
            Message::Choose { target, act } if *target == open.target => {
                *state = None;
                context.request((act.clone(), *target));
            }
            Message::Open(_) | Message::Choose { .. } => return false,
            Message::Page(page) => open.page = *page,
            Message::Close => *state = None,
        }
        true
    }

    fn view_key(_: &State, inputs: &Inputs<'_>) -> ViewKey {
        view::key(inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::render(state, inputs)
    }
}
