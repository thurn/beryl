//! The chapter symbols of a Saga, each with a lore threshold and an effect.

use super::Ability;
use crate::trigger::{Trigger, TriggerEvent};

const CHAPTERS: [Trigger; 255] = {
    let mut chapters = [Trigger::chapters(1, 1); 255];
    let mut index = 0;
    while index < chapters.len() {
        let number = index as u8 + 1;
        chapters[index] = Trigger::chapters(number, number);
        index += 1;
    }
    chapters
};

struct Chapters<const FIRST: u8, const LAST: u8>;

impl<const FIRST: u8, const LAST: u8> Chapters<FIRST, LAST> {
    const TRIGGER: Trigger = {
        assert!(FIRST > 0 && FIRST <= LAST);
        Trigger::chapters(FIRST, LAST)
    };
}

impl Ability {
    /// A Saga chapter symbol (see [CR 715.2b]). Put its instructions in [`Ability::effect`].
    ///
    /// # Panics
    ///
    /// If `number` is zero. Sagas start at chapter I.
    ///
    /// [CR 715.2b]: https://yawgatog.com/resources/magic-rules/#R7152b
    pub const fn chapter(number: u8) -> Ability {
        assert!(number > 0);
        Ability::triggered(&CHAPTERS[number as usize - 1])
    }

    /// Consecutive Saga chapter symbols sharing one paragraph and effect (see [CR 715.2c]).
    /// Each symbol triggers separately when its number is reached.
    ///
    /// [CR 715.2c]: https://yawgatog.com/resources/magic-rules/#R7152c
    pub const fn chapters<const FIRST: u8, const LAST: u8>() -> Ability {
        Ability::triggered(&Chapters::<FIRST, LAST>::TRIGGER)
    }

    /// The first and last chapter numbers of this ability, if it is a chapter ability.
    pub const fn chapter_numbers(&self) -> Option<(u8, u8)> {
        match self.trigger() {
            Some(Trigger {
                event: TriggerEvent::Chapter { first, last },
                ..
            }) => Some((*first, *last)),
            _ => None,
        }
    }
}
