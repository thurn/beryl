//! Typed updates and cached, pure views for parts of the client.

use std::fmt::Debug;

use kit_ui::element::{Action, Element};

/// A screen, overlay or piece of the HUD with its own messages and view.
///
/// State holds only what the session does not know, such as whether the log is
/// folded, a scroll position or a draft. Derive what the latest session view or
/// prompt already tells you in `view`, from its inputs.
pub trait Feature {
    /// Source file in the Beryl 3D workspace that owns this feature.
    const SOURCE: &'static str;

    type State: Default + Debug;
    /// A closed message enum specific to this feature. The router uses its
    /// type to find the owning feature.
    type Message: Clone + PartialEq + Debug + 'static;
    type Inputs<'a>;
    type ViewKey: PartialEq;
    type Command;
    type Request;

    /// Handles a message and reports whether the cached view changed.
    fn update(
        state: &mut Self::State,
        message: &Self::Message,
        context: &mut Context<'_, Self::Command, Self::Request>,
    ) -> bool;

    /// Identifies the inputs read by `view`. An equal key for the same state
    /// must produce an equal element, including its actions.
    fn view_key(state: &Self::State, inputs: &Self::Inputs<'_>) -> Self::ViewKey;

    /// Builds an element using only the state and inputs.
    fn view(state: &Self::State, inputs: Self::Inputs<'_>) -> Element;

    /// Gives an element a message the router can return to this feature.
    fn action(message: Self::Message) -> Action {
        Action::new(message)
    }
}

/// What a feature asks the client to do after its update.
pub enum Effect<Command, Request> {
    Command(Command),
    Request(Request),
    Message(Action),
    FrameAt(u64),
}

type Effects<F> = Vec<Effect<<F as Feature>::Command, <F as Feature>::Request>>;

/// The effects an update can send, in the order it sends them.
pub struct Context<'a, Command, Request>(&'a mut Vec<Effect<Command, Request>>);

impl<Command, Request> Context<'_, Command, Request> {
    pub fn command(&mut self, command: Command) {
        self.0.push(Effect::Command(command));
    }

    pub fn request(&mut self, request: Request) {
        self.0.push(Effect::Request(request));
    }

    pub fn send(&mut self, message: Action) {
        self.0.push(Effect::Message(message));
    }

    /// Asks for a frame at a time in nanoseconds since the client started.
    pub fn frame_at(&mut self, time: u64) {
        self.0.push(Effect::FrameAt(time));
    }
}

/// An update supplied by a feature's owner that can borrow live inputs or return a value.
pub trait StateUpdate<F: Feature> {
    type Output;

    /// Applies the update and reports whether the cached view changed.
    fn apply(self, state: &mut F::State) -> (bool, Self::Output);
}

/// A feature's live state and the last element built for it.
pub struct FeatureSlot<F: Feature> {
    state: F::State,
    cached: Option<(F::ViewKey, Element)>,
}

impl<F: Feature> Default for FeatureSlot<F> {
    fn default() -> Self {
        Self {
            state: F::State::default(),
            cached: None,
        }
    }
}

impl<F: Feature> FeatureSlot<F> {
    #[must_use]
    pub fn state(&self) -> &F::State {
        &self.state
    }

    /// Applies an owner's update, invalidating the view when the update changes it.
    pub fn apply<U: StateUpdate<F>>(&mut self, update: U) -> U::Output {
        let (changed, output) = update.apply(&mut self.state);
        if changed {
            self.cached = None;
        }
        output
    }

    /// Applies one message and returns the effects to the router.
    pub fn update(&mut self, message: &F::Message) -> (bool, Effects<F>) {
        let mut effects = Vec::new();
        let changed = F::update(&mut self.state, message, &mut Context(&mut effects));
        if changed {
            self.cached = None;
        }
        (changed, effects)
    }

    /// Reuses the element until an update changes the state or a view input changes.
    pub fn view(&mut self, inputs: F::Inputs<'_>) -> &Element {
        self.view_with(inputs, F::view)
    }

    /// Caches a view whose mount supplies context local to the build, such as
    /// a copy of a placement scene for recording the view's layout checks.
    pub fn view_with<'a>(
        &mut self,
        inputs: F::Inputs<'a>,
        build: impl FnOnce(&F::State, F::Inputs<'a>) -> Element,
    ) -> &Element {
        let key = F::view_key(&self.state, &inputs);
        if self.cached.as_ref().is_none_or(|(last, _)| *last != key) {
            self.cached = Some((key, build(&self.state, inputs)));
        }
        &self.cached.as_ref().expect("the feature has a view").1
    }
}
