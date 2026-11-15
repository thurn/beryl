use beryl_session::protocol::{ObjectId as Slot, PlayerView};
use beryl_session::script::{Open, Script, Table as Stage, Verb, carried, replay};
use beryl_table::action::Selection;
use beryl_table::state::{Card, Object, ObjectId, Zone};
use beryl_table::{Action, Chance, Change, Log, Seat, Table};
use beryl_table_session::script::{ProposalMarks, script, script_with_proposal_marks};
use beryl_table_session::view::{Slots, view};

pub const HOST: Seat = Seat(0);
pub const GUEST: Seat = Seat(1);
pub const FIELD: Zone = Zone::Battlefield;

pub struct Game {
    pub table: Table,
    pub log: Log,
    chance: Chance,
    pub views: [(PlayerView, Slots); 2],
    spans: [Vec<Open>; 2],
    marks: [Vec<Slot>; 2],
}

impl Game {
    pub fn new(table: Table) -> Self {
        let log = Log::new(&table);
        let views = [HOST, GUEST].map(|seat| {
            let mut slots = Slots::default();
            let (initial, _) = view(&table, &log, seat, &mut slots).unwrap();
            (initial, slots)
        });
        Self {
            table,
            log,
            chance: Chance::fresh(777),
            views,
            spans: Default::default(),
            marks: Default::default(),
        }
    }

    pub fn apply(&mut self, by: Seat, action: &Action) -> [Script; 2] {
        self.try_apply(by, action)
            .unwrap_or_else(|error| panic!("{action:?}: {error:?}"))
    }

    pub fn try_apply(
        &mut self,
        by: Seat,
        action: &Action,
    ) -> Result<[Script; 2], beryl_table::Refusal> {
        self.try_apply_marked(by, action, [Vec::new(), Vec::new()])
    }

    pub fn try_apply_marked(
        &mut self,
        by: Seat,
        action: &Action,
        marks: [Vec<Slot>; 2],
    ) -> Result<[Script; 2], beryl_table::Refusal> {
        let host_before = self.table.clone();
        self.log
            .apply(&mut self.table, by, action, &mut self.chance)?;
        let entry = self.log.entries().last().unwrap();
        let scripts = [HOST, GUEST].map(|seat| {
            let index = usize::from(seat.0);
            let (before, previous) = &mut self.views[index];
            let mut slots = previous.clone();
            let (after, _) = view(&self.table, &self.log, seat, &mut slots).unwrap();
            let script = if self.marks[index].is_empty() && marks[index].is_empty() {
                script(before, entry, &host_before, &after, seat, previous, &slots)
            } else {
                script_with_proposal_marks(
                    before,
                    Some((entry, &host_before)),
                    &after,
                    seat,
                    previous,
                    &slots,
                    ProposalMarks {
                        before: &self.marks[index],
                        after: &marks[index],
                    },
                )
            };
            assert_eq!(
                replay::failure(&Stage::of(before), &script, &after),
                None,
                "{action:?}"
            );
            let start = if script.reset.is_some() {
                &after
            } else {
                &*before
            };
            self.spans[index] = carried(Stage::of(start), &self.spans[index], &script)
                .unwrap_or_else(|error| panic!("{action:?}: {error}\n{script}"));
            *before = after;
            *previous = slots;
            script
        });
        self.marks = marks;
        Ok(scripts)
    }

    #[allow(
        dead_code,
        reason = "only some integration suites check for a single verb"
    )]
    pub fn verbs(&mut self, by: Seat, action: &Action, verb: Verb) {
        for script in self.apply(by, action) {
            assert!(
                script
                    .beats
                    .iter()
                    .flatten()
                    .any(|step| step.act.verb() == verb),
                "{action:?}:\n{script}"
            );
        }
    }
}

pub fn select(zone: Zone, ids: &[u32]) -> Selection {
    Selection {
        zone,
        objects: ids.iter().copied().map(ObjectId).collect(),
    }
}

pub fn card(name: &str) -> Card {
    Card::Custom {
        name: name.into(),
        types: "Creature".into(),
        colors: Vec::new(),
        stats: Some((2, 2)),
    }
}

pub fn add(table: &mut Table, id: u32, name: &str, zone: Zone, known_to: &[Seat]) {
    let mut object = Object::new(ObjectId(id), card(name), HOST, zone);
    object.known_to = known_to.to_vec();
    Change::Create {
        object,
        index: table.zone(zone).unwrap().len(),
    }
    .perform(table);
}
