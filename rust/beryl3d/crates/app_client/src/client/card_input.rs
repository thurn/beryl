//! Delivers released table cards to the manual table, prompt or inspector.

use beryl_protocol::protocol::ObjectId;
use beryl_protocol::script::Subject;

use super::Client;
use app_screens::screens::Screen;
use game_hud::hud::inspector::Message as Inspect;

impl Client {
    /// A press that picked nothing up on the card in `slot`: with Cmd held on the
    /// manual table it adds the card to the selection or takes it out, on a card the open
    /// decision refuses it says why (`refuse_click`), and otherwise it clicks the open prompt on
    /// the card.
    pub(super) fn card_clicked(&mut self, slot: usize) {
        if let Ok(slot) = u16::try_from(slot)
            && !self.table_click(ObjectId(slot))
            && !self.refuse_click(ObjectId(slot))
        {
            let on = Subject::Object {
                slot: ObjectId(slot),
            };
            self.click(beryl_protocol::prompt::Click::On { on });
        }
    }

    /// A finger's tap on the card in `slot`: a click if the open prompt can use the card or the
    /// manual table is shown, and otherwise the card shown whole.
    pub(super) fn card_tapped(&mut self, slot: usize) {
        if self.features.screen != Screen::Game || self.usable(slot) {
            self.card_clicked(slot);
        } else if let Ok(slot) = u16::try_from(slot) {
            self.features.inspect(&Inspect::Pin(Some(ObjectId(slot))));
        }
    }

    /// Whether the open prompt can use the card in `slot`: it marks the card.
    fn usable(&self, slot: usize) -> bool {
        let Some(prompt) = self
            .features
            .prompts
            .current()
            .filter(|_| self.features.layer_open)
        else {
            return false;
        };
        let Ok(slot) = u16::try_from(slot) else {
            return false;
        };
        let on = Subject::Object {
            slot: ObjectId(slot),
        };
        prompt.marks.iter().any(|marked| marked.on == on)
    }
}
