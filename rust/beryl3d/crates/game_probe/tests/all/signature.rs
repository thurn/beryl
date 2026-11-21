use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::prompt::{PartName, PromptView, Purpose, ShapeBody, Words};
use beryl_protocol::protocol::{
    Candidate, CandidateView, DecisionId, DecisionView, Kind, ObjectId, SessionId, Side, Status,
    ZoneName,
};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_probe::signature::{Measure, Signature};
use game_table::table::Window;

fn moment(name: &str, title: &str, candidates: &[u16], tapped: bool) -> GameUpdate {
    let mut builder = ViewBuilder::new();
    builder.card(ZoneName::Battlefield, Side::You, name).tapped = tapped;
    builder.card(ZoneName::Battlefield, Side::Opponent, "Forest");
    let mut update = builder.update();
    let prompt = PromptView {
        revision: 0,
        purpose: Purpose::Choose,
        part: PartName::Pick,
        title: title.to_owned(),
        words: Words::default(),
        count: None,
        about: Vec::new(),
        from: None,
        marks: Vec::new(),
        previews: Vec::new(),
        buttons: Vec::new(),
        body: ShapeBody::Candidates,
    };
    update.view.status = Status::Deciding {
        decision: DecisionView {
            id: DecisionId {
                session: SessionId(1),
                seq: 1,
            },
            kind: Kind::Priority,
            prompt: Box::new(prompt),
            candidates: candidates
                .iter()
                .map(|&slot| CandidateView {
                    label: "Choose this card".to_owned(),
                    option: Candidate::Object {
                        slot: ObjectId(slot),
                    },
                    payments: Vec::new(),
                    blocks: Vec::new(),
                    nth: None,
                    text: None,
                    targets: Vec::new(),
                })
                .collect(),
            resolving: None,
            pass_to: None,
            pass_on: None,
            source: None,
            halt: false,
            cancellable: false,
            undo: 0,
            undo_depth: 0,
            refused: Vec::new(),
            acting_for: None,
        },
    };
    update
}

fn signature(update: &GameUpdate) -> Signature {
    Measure::new(&crate::test_data::data(), &[Window::default()])
        .recording(&[FixtureMessage::game_update(update)])
        .unwrap()
}

#[test]
fn renaming_a_card_without_changing_the_layout_keeps_the_signature() {
    assert_eq!(
        signature(&moment("Island", "Choose Island", &[0], false)),
        signature(&moment("Plains", "Choose Plains", &[0], false))
    );
}

#[test]
fn another_candidate_a_wrapped_title_and_a_tapped_card_each_change_the_signature() {
    let title = "Choose a card";
    let base = signature(&moment("Island", title, &[0], false));
    let wrapped = "Choose a card and read the text in this prompt before choosing another card and confirming the first selection.";
    assert_ne!(base, signature(&moment("Island", title, &[0, 1], false)));
    assert_ne!(base, signature(&moment("Island", wrapped, &[0], false)));
    assert_ne!(base, signature(&moment("Island", title, &[0], true)));
}

#[test]
fn battlefield_twins_and_tucked_cards_change_the_signature() {
    let mut builder = ViewBuilder::new();
    let host = builder
        .card(ZoneName::Battlefield, Side::You, "Island")
        .slot;
    let guest = builder
        .card(ZoneName::Battlefield, Side::You, "Plains")
        .slot;
    let separated = builder.update();
    let mut twins = separated.clone();
    twins.view.objects[usize::from(guest.0)].name = Some("Island".to_owned());
    assert_ne!(signature(&separated), signature(&twins));
    let mut tucked = separated.clone();
    tucked.view.objects[usize::from(guest.0)].attached_to = Some(host);
    assert_ne!(signature(&separated), signature(&tucked));
}

#[test]
fn changing_which_library_card_is_on_top_changes_the_signature() {
    let mut builder = ViewBuilder::new();
    builder.card(ZoneName::Library, Side::You, "Island");
    builder.card(ZoneName::Library, Side::You, "Plains").name = Some("Plains".to_owned());
    let original = builder.update();
    let mut swapped = original.clone();
    swapped.view.players.you.library.swap(0, 1);
    assert_ne!(signature(&original), signature(&swapped));
}

#[test]
fn a_notice_pill_and_a_banner_have_different_signatures() {
    use beryl_protocol::script::{Act, Notice};

    let mut pill = ViewBuilder::new().update();
    pill.view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Announce {
                notice: Notice::Scry {
                    player: Side::You,
                    top: 0,
                    bottom: 1,
                },
            }])
            .build(),
    );
    let mut banner = pill.clone();
    banner.view.script.as_mut().unwrap().beats[0][0].act = Act::Announce {
        notice: Notice::Shuffle,
    };
    assert_ne!(signature(&pill), signature(&banner));
}
