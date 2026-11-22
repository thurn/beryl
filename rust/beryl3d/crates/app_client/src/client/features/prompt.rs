//! Mounts prompt views and forwards their typed commands to the session router.

use std::cell::RefCell;
use std::convert::Infallible;

use beryl_protocol::prompt::{ShapeBody, shape_name};
use game_hud::hud::HudIn;
use game_hud::prompts;
use kit_scene::{Effect, Feature};
use kit_ui::element::{Action, Element};

use super::{Client, Features, Placement, REGISTRY, Registration, Slots, print_state};

pub(super) type Draw = fn(&Slots, &'static str, prompts::Inputs<'_>) -> Element;

impl Registration {
    pub(super) const fn prompt<F: Prompt>(id: &'static str, placement: Placement) -> Self {
        Self {
            owner: std::any::type_name::<F>,
            owns_float: |_| false,
            id,
            source: F::SOURCE,
            placement: Some(placement),
            create: || Box::new(RefCell::new(prompts::Mounted::<F>::default())),
            state: |slots, id| print_state(prompt_slot::<F>(slots, id).borrow().state()),
            route: route_prompt::<F>,
            view: |_, _, _| None,
            prompt_view: Some(|slots, id, inputs| {
                prompt_slot::<F>(slots, id).borrow_mut().view(inputs)
            }),
            shortcuts: &[],
            on_key: super::keys::none,
        }
    }
}

impl Features {
    fn draw_prompt(
        &self,
        placement: Placement,
        inputs: prompts::Inputs<'_>,
        explaining: bool,
    ) -> Option<Element> {
        let entry = REGISTRY
            .iter()
            .find(|entry| entry.placement == Some(placement))?;
        let mut element = (entry.prompt_view?)(&self.slots, entry.id, inputs);
        if explaining && element.probe_owner.is_none() {
            element.probe_owner = Some((entry.owner)());
        }
        Some(element)
    }

    #[cfg(feature = "stories")]
    pub(super) fn named_prompt_view<'a>(
        &'a self,
        placement: Placement,
        mut hud: HudIn<'a>,
    ) -> Option<Element> {
        hud.prompt = self.prompts.current();
        if placement == Placement::PromptBar {
            return self.draw_prompt(placement, prompts::Inputs { hud, bar: None }, false);
        }
        let shown = match &hud.prompt?.body {
            ShapeBody::Candidates => Placement::Candidates,
            body => Placement::Shape(shape_name(body.shape()?)),
        };
        (shown == placement)
            .then(|| self.prompt_view(hud, false))
            .flatten()
    }

    pub(in crate::client) fn prompt_view(
        &self,
        hud: HudIn<'_>,
        explaining: bool,
    ) -> Option<Element> {
        if !prompts::bar::appears(&hud) {
            return None;
        }
        let bar = self.draw_prompt(
            Placement::PromptBar,
            prompts::Inputs { hud, bar: None },
            explaining,
        )?;
        let Some(prompt) = hud.prompt else {
            return Some(bar);
        };
        let placement = match &prompt.body {
            ShapeBody::Candidates => Placement::Candidates,
            body => Placement::Shape(shape_name(body.shape()?)),
        };
        self.draw_prompt(
            placement,
            prompts::Inputs {
                hud,
                bar: Some(&bar),
            },
            explaining,
        )
    }
}

pub(super) trait Prompt:
    for<'a> Feature<
        Inputs<'a> = prompts::Inputs<'a>,
        Command = prompts::Command,
        Request = Infallible,
    > + 'static
{
}

impl<F> Prompt for F where
    F: for<'a> Feature<
            Inputs<'a> = prompts::Inputs<'a>,
            Command = prompts::Command,
            Request = Infallible,
        > + 'static
{
}

fn prompt_slot<'a, F: Prompt>(
    slots: &'a Slots,
    id: &'static str,
) -> &'a RefCell<prompts::Mounted<F>> {
    slots[id]
        .downcast_ref()
        .expect("the registry owns this prompt's slot")
}

fn route_prompt<F: Prompt>(client: &mut Client, id: &'static str, action: &Action) -> Option<bool> {
    let message = action.downcast_ref::<F::Message>()?;
    let (mut changed, effects) = prompt_slot::<F>(&client.features.slots, id)
        .borrow_mut()
        .update(message);
    for effect in effects {
        match effect {
            Effect::Command(prompts::Command::Click(click)) => client.click(click),
            Effect::Command(prompts::Command::Undo) => client.undo(),
            Effect::Request(never) => match never {},
            Effect::Message(action) => {
                changed |= client.route_feature(&action).unwrap_or(false);
            }
            Effect::FrameAt(at) => client.features.request_frame(at),
        }
    }
    Some(changed)
}
