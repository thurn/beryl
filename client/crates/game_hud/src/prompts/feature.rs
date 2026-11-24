//! The inputs to a prompt feature and the shared routing for its clicks.

use std::marker::PhantomData;

use beryl_protocol::prompt::{Click, PromptView};
use beryl_protocol::script::Subject;
use game_table::table::TraySpace;
use kit_base::data::profiles::{FloatingPrompt, Piece};
use kit_base::geometry::{Rect, Window};
use kit_frame::frame::{Clearance, ElementId};
use kit_scene::{Feature, FeatureSlot};
use kit_ui::element::Element;
use kit_ui::input::FaceId;

use crate::hud::{
    HudIn,
    placement_scene::{Attempt, PlacementScene},
};

/// Session inputs and the already drawn bar, which shapes can add controls around.
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub hud: HudIn<'a>,
    pub bar: Option<&'a Element>,
}

/// Projected inputs the prompt's view reads. Store these with its element
/// instead of retaining a copy of the game view or the match's face list.
#[derive(PartialEq)]
pub struct ViewKey {
    window: Window,
    slots: [Option<Rect>; 10],
    floating: Option<FloatingPrompt>,
    show_phase: bool,
    target_points: f32,
    text_points: Option<f32>,
    tray: TraySpace,
    prompt: Option<PromptView>,
    placement: Option<Vec<(ElementId, Option<u32>, Rect)>>,
    bar: Option<Element>,
    details: Details,
    recording: bool,
}

#[derive(Default, PartialEq)]
pub(super) enum Details {
    #[default]
    None,
    Bar {
        acting_for: bool,
        opponent_acts: bool,
        stack_empty: Option<bool>,
    },
    Ways(Vec<Option<FaceId>>),
    Order {
        items: Vec<(String, Option<FaceId>)>,
        carried: Option<Subject>,
    },
    Divide(Vec<bool>),
}

impl ViewKey {
    pub(super) fn new(inputs: &Inputs<'_>, details: Details) -> Self {
        let hud = &inputs.hud;
        let placement = matches!(
            details,
            Details::Bar { .. } | Details::Ways(_) | Details::Divide(_)
        )
        .then(|| hud.placement_scene.map(PlacementScene::prompt_geometry))
        .flatten();
        Self {
            window: hud.window,
            slots: hud
                .profile
                .slots([
                    "prompt.bar",
                    "prompt.dock",
                    "prompt.stacked",
                    "prompt.clearance",
                    "prompt.asked",
                    "phase",
                    "prompt.ways",
                    "prompt.catalog",
                    "divide.edge",
                    "divide.side",
                ])
                .map(|slot| slot.map(|slot| slot.rect)),
            floating: hud.profile.data().hud.floating_prompt,
            show_phase: hud.profile.data().hud.table.contains(&Piece::Phase),
            target_points: hud.profile.data().checks.target_points,
            text_points: hud.profile.data().checks.text_points,
            tray: if matches!(details, Details::Order { .. }) {
                hud.profile.order_tray()
            } else {
                hud.profile.tray()
            },
            prompt: hud.prompt.cloned(),
            placement,
            bar: inputs.bar.cloned(),
            details,
            recording: hud.placement_scene.is_some_and(PlacementScene::recording),
        }
    }
}

/// A message from the controls of one shape. Its type identifies the feature
/// even when another shape sends the same wire click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message<F>(pub Click, PhantomData<F>);

impl<F> Message<F> {
    pub const fn click(click: Click) -> Self {
        Self(click, PhantomData)
    }
}

/// Effects that prompt controls pass to the session router.
pub enum Command {
    Click(Click),
    Undo,
}

type Update<F> = (
    bool,
    Vec<kit_scene::Effect<<F as Feature>::Command, <F as Feature>::Request>>,
);

/// A prompt view's cached element and the placement checks it reports.
pub struct Mounted<F: for<'a> Feature<Inputs<'a> = Inputs<'a>>> {
    slot: FeatureSlot<F>,
    clearances: Vec<Clearance>,
    attempts: Vec<Attempt>,
}

impl<F: for<'a> Feature<Inputs<'a> = Inputs<'a>>> Default for Mounted<F> {
    fn default() -> Self {
        Self {
            slot: FeatureSlot::default(),
            clearances: Vec::new(),
            attempts: Vec::new(),
        }
    }
}

impl<F: for<'a> Feature<Inputs<'a> = Inputs<'a>>> Mounted<F> {
    #[must_use]
    pub fn state(&self) -> &F::State {
        self.slot.state()
    }

    pub fn update(&mut self, message: &F::Message) -> Update<F> {
        self.slot.update(message)
    }

    pub fn view(&mut self, inputs: Inputs<'_>) -> Element {
        let element = self.slot.view_with(inputs, |state, inputs| {
            // The live scene collects diagnostic relations from several features.
            // Solve this view against a copy of its table geometry so it reads
            // only its own inputs, and retain the checks along with the element.
            let placement = inputs.hud.placement_scene.map(PlacementScene::prompt_scene);
            let element = F::view(
                state,
                Inputs {
                    hud: HudIn {
                        placement_scene: placement.as_ref(),
                        ..inputs.hud
                    },
                    bar: inputs.bar,
                },
            );
            self.attempts = placement
                .as_ref()
                .map_or_else(Vec::new, PlacementScene::take_attempts);
            self.clearances = placement.map_or_else(Vec::new, PlacementScene::take_clearances);
            element
        });
        if let Some(scene) = inputs.hud.placement_scene {
            scene.add_clearances(&self.clearances);
            scene.add_attempts(&self.attempts);
        }
        element.clone()
    }
}

pub(super) fn bar_only(_prompt: &PromptView, _hud: &HudIn<'_>, bar: Element) -> Element {
    bar
}

macro_rules! prompt_feature {
    ($feature:ident, $view:path, $own:path $(, $details:path)?) => {
        /// The controls and presentation for this prompt shape.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $feature;

        impl $feature {
            #[must_use]
            pub const fn message(click: beryl_protocol::prompt::Click) -> crate::prompts::feature::Message<Self> {
                crate::prompts::feature::Message::click(click)
            }

            pub(super) fn own(
                shown: Option<crate::prompts::Shown<'_>>,
                was: Option<crate::prompts::Shown<'_>>,
                steps: &mut crate::prompts::Steps<'_>,
            ) {
                $own(shown, was, steps);
            }
        }

        impl kit_scene::Feature for $feature {
            const SOURCE: &'static str = file!();

            // The session owns every selection and sends the resulting controls and marks.
            type State = ();
            type Message = crate::prompts::feature::Message<Self>;
            type Inputs<'a> = crate::prompts::feature::Inputs<'a>;
            type ViewKey = crate::prompts::feature::ViewKey;
            type Command = crate::prompts::feature::Command;
            type Request = std::convert::Infallible;

            fn update(
                (): &mut (),
                message: &Self::Message,
                context: &mut kit_scene::Context<'_, Self::Command, Self::Request>,
            ) -> bool {
                context.command(Self::Command::Click(message.0));
                false
            }

            fn view_key((): &(), inputs: &Self::Inputs<'_>) -> Self::ViewKey {
                Self::ViewKey::new(inputs, prompt_feature!(@details inputs $(, $details)?))
            }

            fn view((): &(), inputs: Self::Inputs<'_>) -> kit_ui::element::Element {
                let prompt = inputs.hud.prompt.expect("only the open prompt's shape is mounted");
                let bar = inputs.bar.expect("the bar is drawn before the shape").clone();
                let mut element = $view(prompt, &inputs.hud, bar);
                if !matches!(prompt.body, beryl_protocol::prompt::ShapeBody::YesNo { .. })
                    && crate::prompts::Piles::of(prompt).is_some()
                {
                    element.children.insert(0, crate::prompts::shade(&inputs.hud));
                }
                element
            }
        }
    };
    (@details $inputs:ident, $details:path) => { $details(&$inputs.hud) };
    (@details $inputs:ident) => { crate::prompts::feature::Details::None };
}
