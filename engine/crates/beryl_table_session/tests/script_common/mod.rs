use beryl_protocol::protocol::ObjectId as Slot;
use beryl_protocol::script::{Open, Script, Table as Stage, Verb, carried, replay};
use beryl_table::action::Selection;
use beryl_table::state::{Card, Object, ObjectId, Zone};
use beryl_table::{Action, Chance, Change, Log, Seat, Table};
use beryl_table_session::publication::{ProposalMarks, PublishedTable, SeatPublication};

pub const HOST: Seat = Seat(0);
pub const GUEST: Seat = Seat(1);
pub const FIELD: Zone = Zone::Battlefield;

pub struct Game {
    pub published: PublishedTable,
    spans: [Vec<Open>; 2],
    marks: [Vec<Slot>; 2],
}

impl Game {
    pub fn new(table: Table) -> Self {
        let log = Log::new(&table);
        Self {
            published: PublishedTable::new(table, log, Chance::fresh(777)).unwrap(),
            spans: Default::default(),
            marks: Default::default(),
        }
    }

    pub fn views(&self) -> [&SeatPublication; 2] {
        [HOST, GUEST].map(|seat| self.published.publication(seat).unwrap())
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
        let before = self
            .views()
            .map(|publication| Stage::of(publication.view()));
        self.published.apply_host_with_marks(by, action, |seat| {
            let index = usize::from(seat.0);
            ProposalMarks {
                before: &self.marks[index],
                after: &marks[index],
            }
        })?;
        let scripts = [HOST, GUEST].map(|seat| {
            let index = usize::from(seat.0);
            let after = self.published.publication(seat).unwrap().view();
            let script = after.script.as_ref().unwrap();
            assert_eq!(
                replay::failure(&before[index], script, after),
                None,
                "{action:?}"
            );
            let start = if script.reset.is_some() {
                Stage::of(after)
            } else {
                before[index].clone()
            };
            self.spans[index] = carried(start, &self.spans[index], script)
                .unwrap_or_else(|error| panic!("{action:?}: {error}\n{script}"));
            script.clone()
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
