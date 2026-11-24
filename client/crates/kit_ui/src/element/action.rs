//! Messages carried by elements and handed to their owning feature.

use std::any::Any;
use std::fmt::{self, Debug};
use std::mem::{align_of, needs_drop, size_of};
use std::rc::Rc;

use smallbox::{SmallBox, smallbox, space::S4};

/// A message that can be cloned, compared by value and printed without naming its type.
/// Small messages, such as a button id, are stored inline.
pub struct Action(MessageBox);

type MessageBox = SmallBox<dyn Message, S4>;

trait Message: Debug {
    fn value(&self) -> &dyn Any;
    fn equals(&self, other: &dyn Message) -> bool;
    fn clone_box(&self) -> MessageBox;
}

impl<T: Clone + PartialEq + Debug + 'static> Message for T {
    fn value(&self) -> &dyn Any {
        self
    }

    fn equals(&self, other: &dyn Message) -> bool {
        other.value().downcast_ref::<T>() == Some(self)
    }

    fn clone_box(&self) -> MessageBox {
        smallbox!(self.clone())
    }
}

/// A message that exceeds inline storage or owns resources that could be costly to clone.
struct SharedMessage(Rc<dyn Message>);

impl Debug for SharedMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&*self.0, formatter)
    }
}

impl Message for SharedMessage {
    fn value(&self) -> &dyn Any {
        self.0.value()
    }

    fn equals(&self, other: &dyn Message) -> bool {
        self.0.equals(other)
    }

    fn clone_box(&self) -> MessageBox {
        smallbox!(Self(Rc::clone(&self.0)))
    }
}

impl Action {
    /// Stores a message for a press or keyboard shortcut.
    pub fn new<T: Clone + PartialEq + Debug + 'static>(message: T) -> Self {
        let storage: MessageBox = if !needs_drop::<T>()
            && size_of::<T>() <= size_of::<S4>()
            && align_of::<T>() <= align_of::<S4>()
        {
            smallbox!(message)
        } else {
            smallbox!(SharedMessage(Rc::new(message)))
        };
        Self(storage)
    }

    /// Reads the message if the caller knows its type.
    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.0.value().downcast_ref()
    }
}

impl Clone for Action {
    fn clone(&self) -> Self {
        Self(self.0.clone_box())
    }
}

impl PartialEq for Action {
    fn eq(&self, other: &Self) -> bool {
        self.0.equals(&*other.0)
    }
}

impl Debug for Action {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&*self.0, formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::Action;

    #[test]
    fn actions_compare_and_print_their_typed_values() {
        let count = Action::new(4_u16);
        assert_eq!(count, Action::new(4_u16));
        assert_ne!(count, Action::new(4_u32));
        assert_eq!(count.downcast_ref::<u16>(), Some(&4));
        assert_eq!(format!("{count:?}"), "4");

        let left = Action::new(String::from("confirm"));
        let equal = Action::new(String::from("confirm"));
        assert_eq!(left, equal);
        assert_eq!(left, left.clone());
        assert_ne!(left, Action::new(String::from("cancel")));
        assert_ne!(left, Action::new("confirm"));
        assert_eq!(
            left.downcast_ref::<String>().map(String::as_str),
            Some("confirm")
        );
        assert_eq!(format!("{left:?}"), r#""confirm""#);
    }
}
