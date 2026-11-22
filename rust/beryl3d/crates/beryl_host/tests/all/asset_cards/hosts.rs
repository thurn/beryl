//! Play three screens through the desktop frame loop with an offscreen target and real
//! workers. A stub atlas would let the start and result screens pass with missing letters.

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_client::{Client, Config, Input};
use beryl_art::Origins;
use beryl_card_records::{CardRecordsRequest, TokenMakers, card_records};
use beryl_host::{Chosen, Feed, Scans, Ticked, Windowless, WindowlessOptions, match_faces};
use beryl_protocol::GameUpdate;
use beryl_protocol::card_records::CardRecords;
use beryl_protocol::protocol::{CardType, LossReason, OutcomeView, Side, Status, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_table::faces::FaceList;
use kit_frame::frame::Role;
use kit_ui::input::Size;

use crate::test_scans::{Scratch, scan};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Screen {
    #[default]
    Start,
    Game,
    Result,
    Finished,
}

#[derive(Default, Debug)]
struct Progress {
    screen: Screen,
    found: bool,
    missing_glyphs: usize,
    undrawn: Vec<String>,
    faceless: usize,
}

struct Screens {
    progress: Arc<Mutex<Progress>>,
    records: Option<CardRecords>,
    game: Option<GameUpdate>,
    result: Option<GameUpdate>,
}

impl Feed for Screens {
    fn after(&mut self, client: &mut Client, _ticked: &Ticked) {
        let mut progress = self.progress.lock().unwrap();
        let frame = client.last_frame();
        let shows = |role, name: &str| {
            frame
                .resolved
                .iter()
                .any(|element| element.role == role && frame.name(element.name) == name)
        };
        progress.found = match progress.screen {
            Screen::Start => shows(Role::Region, "start"),
            Screen::Game => shows(Role::Card, "Island"),
            Screen::Result => shows(Role::Text, "Victory"),
            Screen::Finished => return,
        };
        progress.missing_glyphs = client.missing_glyphs();
        progress.undrawn = frame
            .undrawn
            .iter()
            .map(|undrawn| {
                let element = &frame.resolved[undrawn.element as usize];
                format!("{}: {:?}", frame.name(element.name), undrawn.missing)
            })
            .collect();
        progress.faceless = frame.faceless_cards();
        if !progress.found
            || progress.missing_glyphs != 0
            || !progress.undrawn.is_empty()
            || progress.faceless != 0
        {
            return;
        }
        progress.screen = match progress.screen {
            Screen::Start => {
                client.handle(Input::Cards(Box::new(self.records.take().unwrap())));
                client.handle(Input::Update(Box::new(self.game.take().unwrap())));
                Screen::Game
            }
            Screen::Game => {
                client.handle(Input::Update(Box::new(self.result.take().unwrap())));
                Screen::Result
            }
            Screen::Result | Screen::Finished => Screen::Finished,
        };
    }

    fn ended(&self) -> bool {
        self.progress.lock().unwrap().screen == Screen::Finished
    }
}

#[test]
fn the_windowless_frame_loop_draws_start_game_and_result_with_real_assets() {
    let dir = Scratch::new("host");
    let scans = Scans::in_dir(&dir.0);
    let records = card_records(
        &CardRecordsRequest {
            names: vec!["Island".to_owned()],
        },
        TokenMakers::NONE,
    );
    let listed = FaceList::from_records(&records);
    assert!(listed.find("Island").is_some());
    let bytes = scan();
    for face in match_faces(&listed).faces {
        let file = scans.art.join(face.scan.trim_start_matches('/'));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, &bytes).unwrap();
    }
    let mut view = ViewBuilder::new();
    view.card(ZoneName::Battlefield, Side::You, "Island").types = vec![CardType::Land];
    view.script(ScriptBuilder::reset().build());
    let game = view.update();
    let mut result = view.update();
    result.view.status = Status::Over {
        outcome: OutcomeView::Win {
            winner: Side::You,
            reason: LossReason::Concede,
            by: None,
        },
    };
    let progress = Arc::new(Mutex::new(Progress::default()));
    let feed = Screens {
        progress: Arc::clone(&progress),
        records: Some(records),
        game: Some(game),
        result: Some(result),
    };
    let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
    client.handle(Input::Agents(Vec::new()));
    Windowless::new(WindowlessOptions {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 2.0,
        hz: 60.0,
        pause_at: None,
        // Only catches a missing delivery or a stalled loop.
        end_after: Some(Duration::from_mins(2)),
        slow_gpu: Duration::ZERO,
    })
    .with_scans(
        scans,
        Origins {
            cards: "http://127.0.0.1:9",
            backs: "http://127.0.0.1:9",
        },
    )
    .run(client, Box::new(feed), |_, _| Chosen::BASELINE)
    .expect("run the desktop frame loop offscreen");
    let progress = progress.lock().unwrap();
    assert_eq!(progress.screen, Screen::Finished, "{progress:#?}");
}
