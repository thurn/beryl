//! Mounts the table's features and maps effects to the client and session.

use kit_base::data::profiles::SlotName;
use std::convert::Infallible;

use beryl_protocol::protocol::{Answer, TableAction};
use game_manual::features::{
    ask, context_menu, library_order, lobby, log, palette, proposals, table, tokens,
};
use game_manual::manual::{ask::dock_anchor, assists::proposal_bounds, link, opening, turn};
use kit_ui::widgets::FieldState;

use super::{
    Client, Features, Mounted, Placement, Screen, SessionCommand, SlotOwner, ViewInputs, update,
};
use kit_ui::element::Element;

pub(super) use game_manual::features::{
    ask::AskPrompt, context_menu::ContextMenu, library_order::OrderTray, lobby::LobbyScreen,
    log::TableLog, palette::CommandList, proposals::ProposalPanel, table::TableScreen,
    tokens::TokenSearch,
};

impl Features {
    pub(crate) fn table(&self) -> &table::State {
        self.state_of::<TableScreen>()
    }

    pub(in crate::client) fn update_table<U: kit_scene::StateUpdate<TableScreen>>(
        &mut self,
        update: U,
    ) -> U::Output {
        self.slot::<TableScreen>().apply(update)
    }

    pub(in crate::client) fn highlight_token(&mut self, input: tokens::Highlight<'_>) {
        self.slot::<TokenSearch>().apply(input);
    }

    pub(crate) fn menu(&self) -> &context_menu::State {
        self.state_of::<ContextMenu>()
    }

    pub(in crate::client) fn clear_menu(&mut self) {
        self.slot::<ContextMenu>()
            .update(&context_menu::Message::Close);
    }

    pub(crate) fn palette(&self) -> &palette::State {
        self.state_of::<CommandList>()
    }

    pub(crate) fn ask(&self) -> &ask::State {
        self.state_of::<AskPrompt>()
    }

    pub(crate) fn tokens(&self) -> &tokens::State {
        self.state_of::<TokenSearch>()
    }

    pub(crate) fn order(&self) -> &library_order::State {
        self.state_of::<OrderTray>()
    }

    pub(crate) fn lobby(&self) -> &lobby::State {
        self.state_of::<LobbyScreen>()
    }

    pub(crate) fn overlay_open(&self) -> bool {
        self.palette().is_some()
            || self.ask().is_some()
            || self.tokens().search.is_some()
            || self.tokens().custom.is_some()
            || self.order().is_some()
            || self.table().quitting
    }

    pub(crate) fn close_overlay(&mut self) {
        self.slot::<CommandList>().update(&palette::Message::Reset);
        self.slot::<AskPrompt>().update(&ask::Message::Reset);
        self.slot::<TokenSearch>().update(&tokens::Message::Reset);
        self.slot::<OrderTray>()
            .update(&library_order::Message::Reset);
        self.update_table(table::Change::QuitPrompt(false));
    }
}

impl Client {
    pub(in crate::client) fn update_lobby(&mut self, message: &lobby::Message) -> bool {
        update::<LobbyScreen>(self, message)
    }

    pub(in crate::client) fn update_table_log(&mut self, message: &log::Message) {
        update::<TableLog>(self, message);
    }

    pub(in crate::client) fn update_palette(&mut self, message: &palette::Message) {
        update::<CommandList>(self, message);
    }

    pub(in crate::client) fn update_ask(&mut self, message: &ask::Message) {
        update::<AskPrompt>(self, message);
    }

    pub(in crate::client) fn update_tokens(&mut self, message: &tokens::Message) -> bool {
        update::<TokenSearch>(self, message)
    }

    pub(in crate::client) fn update_order(&mut self, message: &library_order::Message) -> bool {
        update::<OrderTray>(self, message)
    }

    pub(in crate::client) fn update_table_menu(&mut self, message: &context_menu::Message) -> bool {
        update::<ContextMenu>(self, message)
    }
}

impl Mounted for TableScreen {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let log = features.view(Placement::TableLog, inputs);
        let proposals = features.view(Placement::TableProposals, inputs);
        let menu = features.view(Placement::TableMenu, inputs);
        let overlay = features.view(Placement::TableOverlay, inputs);
        let overlay_open = features.overlay_open();
        Some(features.slot::<Self>().view(table::Inputs {
            drag_threshold: inputs.drag_threshold,
            hud: *inputs.hud,
            fields: inputs.fields,
            bindings: inputs.bindings,
            link_error: inputs.link_error,
            next_slot: turn::next_slot(inputs.hud),
            log,
            proposals,
            menu,
            overlay,
            overlay_open,
        }))
    }

    fn command(client: &mut Client, answer: Answer) {
        client.send_answer(answer);
    }

    fn request(client: &mut Client, request: table::Request) -> bool {
        match request {
            table::Request::Shortcut(index, shift) => client.table_key(index, shift),
            table::Request::Step { at, active } => {
                if let Some(update) = client.show.latest() {
                    let id = game_manual::manual::turn::step_command(&update.view, at, active);
                    client.run_table_command(id, game_manual::manual::Target::Table);
                }
            }
            table::Request::Next => {
                client.run_table_command("phase.next", game_manual::manual::Target::Table);
            }
            table::Request::Counter { side, at, add } => {
                let pressed = if add {
                    table::Message::Counter { side, at }
                } else {
                    table::Message::CounterRemove { side, at }
                };
                client.adjust_counter(side, at, add, pressed);
            }
            table::Request::Quit => client.requests.push_back(crate::command::Request::Quit),
            table::Request::CloseOverlay => client.close_table_overlay(),
            table::Request::SortCard(card) => {
                let answer = client.show.latest().and_then(|update| {
                    game_manual::manual::assists::sort_answer(&update.view, card)
                });
                if let Some(answer) = answer {
                    client.send_answer(answer);
                }
            }
            table::Request::Leave => client.leave_table(),
            table::Request::Viewer(control) => client.viewer_control(control),
        }
        false
    }
}

impl Mounted for LobbyScreen {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let view = features.lobby_view.as_ref();
        Some(Self::slot_mut(&mut features.slots).view(lobby::Inputs {
            window: inputs.window,
            column: inputs.profile.screen_column(),
            fields: inputs.fields,
            view,
        }))
    }

    fn command(client: &mut Client, command: lobby::Command) {
        client.commands.push_back(match command {
            lobby::Command::Change(change) => SessionCommand::TableLobby(change),
            lobby::Command::SavedTables => SessionCommand::TableSaved,
            lobby::Command::Host { name, link } => SessionCommand::TableHost { name, link },
            lobby::Command::Join { invite, name } => SessionCommand::TableJoin { invite, name },
            lobby::Command::Resume(id) => SessionCommand::TableResume(id),
            lobby::Command::SavedDecks => SessionCommand::DeckList,
            lobby::Command::Import(request) => SessionCommand::DeckImport(request),
        });
    }

    fn request(client: &mut Client, request: lobby::Request) -> bool {
        match request {
            lobby::Request::OpenLobby => {
                client.features.lobby_view = None;
                client.update_start(&app_screens::screens::start::Message::Leave);
                client.features.screen = Screen::Lobby;
                client.change_sheet(None);
            }
            lobby::Request::CopyInvite => {
                if let Some(invite) = client
                    .features
                    .lobby_view
                    .as_ref()
                    .and_then(|view| view.invite.as_ref())
                {
                    client
                        .requests
                        .push_back(crate::command::Request::Copy(invite.clone()));
                }
            }
            lobby::Request::Leave => client.leave_table(),
            lobby::Request::ClearFields(fields) => {
                for field in fields {
                    client.layer.set_field(field, FieldState::default());
                }
            }
        }
        false
    }
}

impl Mounted for ContextMenu {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let slot = features.slot::<Self>();
        slot.state().as_ref()?;
        let face = context_menu::sheet_face(slot.state(), inputs.hud);
        Some(slot.view(context_menu::Inputs {
            window: inputs.window,
            profile: inputs.profile,
            face,
        }))
    }

    fn command(_: &mut Client, command: Infallible) {
        match command {}
    }

    fn request(client: &mut Client, (act, target): Self::Request) -> bool {
        client.give(act, target, false);
        false
    }
}

impl Mounted for CommandList {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let slot = features.slot::<Self>();
        slot.state().as_ref()?;
        Some(slot.view(palette::Inputs {
            metrics: inputs.hud.data.flat.manual.palette,
            fields: inputs.fields,
            window: inputs.window.size,
        }))
    }

    fn command(_: &mut Client, command: Infallible) {
        match command {}
    }

    fn request(client: &mut Client, request: palette::Request) -> bool {
        let shift = client.features.table().modifiers.shift;
        match request {
            palette::Request::Choose(at) => client.run_palette_row(usize::from(at), shift),
            palette::Request::CancelCapture => client.stop_capture(None),
            palette::Request::ClearFocus => client.layer.focus(None),
        }
        false
    }
}

impl Mounted for AskPrompt {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let slot = features.slot::<Self>();
        let open = slot.state().as_ref()?;
        let anchor = dock_anchor(open, inputs.profile, inputs.hud.placement_scene);
        Some(slot.view(ask::Inputs {
            inset: inputs.hud.data.flat.manual.edge,
            fields: inputs.fields,
            window: inputs.window,
            profile: inputs.profile,
            anchor,
        }))
    }

    fn command(_: &mut Client, command: Infallible) {
        match command {}
    }

    fn request(client: &mut Client, request: ask::Request) -> bool {
        match request {
            ask::Request::Confirm => client.confirm_ask(),
            ask::Request::Step(up) => client.step_ask(up),
            ask::Request::Suggestion(at) => client.ask_suggestion(at),
            ask::Request::ClearFocus => client.layer.focus(None),
        }
        false
    }
}

impl Mounted for TokenSearch {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let slot = features.slot::<Self>();
        if slot.state().custom.is_none() {
            inputs.view?;
            slot.state().search.as_ref()?;
        }
        Some(slot.view(tokens::Inputs {
            metrics: inputs.hud.data.flat.manual.palette,
            view: inputs.view,
            fields: inputs.fields,
            window: inputs.window.size,
        }))
    }

    fn command(client: &mut Client, text: String) {
        client.commands.push_back(SessionCommand::CardSearch(text));
    }

    fn request(client: &mut Client, request: tokens::Message) -> bool {
        let shift = client.features.table().modifiers.shift;
        match request {
            tokens::Message::Choose(at) => client.choose_token(usize::from(at), shift),
            tokens::Message::Create => client.create_custom(shift),
            tokens::Message::Close => client.layer.focus(None),
            _ => {}
        }
        false
    }
}

impl Mounted for OrderTray {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let slot = features.slot::<Self>();
        slot.state().as_ref()?;
        Some(slot.view(library_order::Inputs {
            view: inputs.view?,
            window: inputs.window.size,
            card: inputs.profile.card(),
        }))
    }

    fn command(_: &mut Client, command: Infallible) {
        match command {}
    }

    fn request(client: &mut Client, request: library_order::Request) -> bool {
        match request {
            library_order::Request::Done(order) => client.order_done(&order),
            library_order::Request::ClearFocus => client.layer.focus(None),
        }
        false
    }
}

impl Mounted for TableLog {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let view = inputs.view?;
        let lines = &view.table.as_ref()?.log;
        if lines.is_empty() || link::frozen(view) || opening::prompt(view).is_some() {
            return None;
        }
        let placement = if let Some(slot) = inputs.profile.slot(SlotName::ManualLog) {
            log::Placement::Folded(slot.rect)
        } else {
            let [toggle, panel] = inputs.profile.slots([SlotName::LogToggle, SlotName::Log]);
            log::Placement::Column {
                toggle: toggle?.rect,
                panel: panel?.rect,
            }
        };
        Some(features.slot::<Self>().view(log::Inputs {
            lines,
            placement,
            metrics: inputs.hud.data.flat.manual.log,
        }))
    }

    fn command(client: &mut Client, command: TableAction) {
        client.send_table(vec![command]);
    }

    fn request(_: &mut Client, request: Infallible) -> bool {
        match request {}
    }
}

impl Mounted for ProposalPanel {
    fn mounted_view<'a>(features: &'a mut Features, inputs: ViewInputs<'_>) -> Option<&'a Element> {
        let view = inputs.view?;
        let proposals = &view.table.as_ref()?.proposals;
        if proposals.is_empty() || link::frozen(view) || opening::prompt(view).is_some() {
            return None;
        }
        let slot = features.slot::<Self>();
        let bounds = proposal_bounds(
            proposals,
            *slot.state(),
            inputs.profile,
            inputs.hud.placement_scene,
        );
        Some(slot.view(proposals::Inputs {
            metrics: inputs.hud.data.flat.manual,
            proposals,
            window: inputs.window,
            profile: inputs.profile,
            bounds,
        }))
    }

    fn command(client: &mut Client, answer: Answer) {
        client.send_answer(answer);
    }

    fn request(_: &mut Client, request: Infallible) -> bool {
        match request {}
    }
}
