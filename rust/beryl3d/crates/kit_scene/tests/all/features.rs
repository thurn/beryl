use std::cell::Cell;
use std::convert::Infallible;

use kit_scene::{Context, Feature, FeatureSlot};
use kit_ui::element::{Element, text};
use kit_ui::layout::Layer;

struct Counter;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Message {
    Add,
}

impl Feature for Counter {
    type State = u8;
    type Message = Message;
    type Inputs<'a> = (&'a Cell<u8>, &'a str);
    type ViewKey = String;
    type Command = Infallible;
    type Request = Infallible;

    fn update(
        state: &mut u8,
        message: &Message,
        _: &mut Context<'_, Infallible, Infallible>,
    ) -> bool {
        match message {
            Message::Add => *state += 1,
        }
        true
    }

    fn view_key(_: &u8, (_, label): &Self::Inputs<'_>) -> String {
        (*label).to_owned()
    }

    fn view(state: &u8, (builds, label): Self::Inputs<'_>) -> Element {
        builds.set(builds.get() + 1);
        text(&format!("{label}: {state}")).on_press(Message::Add)
    }
}

#[test]
fn a_message_rebuilds_the_view_once_and_an_unchanged_view_takes_no_layout() {
    let builds = Cell::new(0);
    let mut feature = FeatureSlot::<Counter>::default();
    let mut layer = Layer::new();
    let initial = feature.view((&builds, "Count"));
    assert_eq!(initial.name, "Count: 0");
    let action = initial.press.clone().expect("the element sends a message");
    layer.show(Some(initial.clone()));
    assert!(layer.take_work().0 > 0);

    let message = action.downcast_ref::<Message>().expect("a counter action");
    let (changed, effects) = feature.update(message);
    assert!(changed);
    assert!(effects.is_empty());
    let updated = feature.view((&builds, "Count"));
    assert_eq!(updated.name, "Count: 1");
    layer.show(Some(updated.clone()));
    assert!(layer.take_work().0 > 0);
    assert_eq!(builds.get(), 2);

    layer.show(Some(feature.view((&builds, "Count")).clone()));
    assert_eq!(builds.get(), 2);
    assert_eq!(layer.take_work(), (0, 0));

    let changed_input = feature.view((&builds, "Total"));
    assert_eq!(changed_input.name, "Total: 1");
    assert_eq!(builds.get(), 3);
}
