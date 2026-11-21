//! The panel for reviewing and applying a table proposal.

mod view;
pub use view::{Inputs, ViewKey};

use beryl_protocol::protocol::Answer;
use kit_scene::{Context, Feature};
use kit_ui::element::Element;
use std::convert::Infallible;

pub type State = Option<u32>;

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Toggle(Option<u32>),
    Line { id: u32, line: u32, on: bool },
    Apply { id: u32, revision: u32 },
}

pub struct ProposalPanel;

impl Feature for ProposalPanel {
    type State = State;
    type Message = Message;
    type Inputs<'a> = Inputs<'a>;
    type ViewKey = ViewKey;
    type Command = Answer;
    type Request = Infallible;

    fn update(
        state: &mut State,
        message: &Message,
        context: &mut Context<'_, Self::Command, Self::Request>,
    ) -> bool {
        match *message {
            Message::Toggle(open) => *state = open,
            Message::Line { id, line, on } => {
                context.command(Answer::ProposalLine { id, line, on });
                return false;
            }
            Message::Apply { id, revision } => {
                *state = None;
                context.command(Answer::ApplyProposal { id, revision });
            }
        }
        true
    }

    fn view_key(_: &State, inputs: &Inputs<'_>) -> ViewKey {
        view::key(inputs)
    }

    fn view(state: &State, inputs: Inputs<'_>) -> Element {
        view::render(*state, inputs)
    }
}
