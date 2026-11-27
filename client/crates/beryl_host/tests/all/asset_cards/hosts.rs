//! Play three screens through the desktop frame loop with an offscreen target and real
//! workers. A stub atlas would let the start and result screens pass with missing letters.

use std::fs;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use app_client::driver::{Page, Recording};
use app_client::{Client, Config, Input};
use beryl_art::Origins;
use beryl_assets::{Upload, light};
use beryl_card_records::{CardRecordsRequest, TokenMakers, card_records, fixture_message};
use beryl_host::{
    Chosen, Feed, Scans, Ticked, Windowless, WindowlessOptions, match_faces, spawn_glyph_worker,
};
use beryl_protocol::GameUpdate;
use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, LossReason, OutcomeView, Side, Status, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use beryl_render::{Limits, Renderer, Target};
use game_table::faces::FaceList;
use kit_base::atlas::GlyphKey;
use kit_frame::frame::{Material, Role};
use kit_text::text::fonts::Font;
use kit_ui::input::Size;
use swash::FontRef;

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

#[test]
fn opening_the_retina_library_draws_every_glyph_after_atlas_growth() {
    const SCALE: f32 = 2.0;
    const ORIGINAL_ATLAS_PAGES: u16 = 8;
    const TITLE: &str = "text \"Library\"";
    const COUNT: &str = "text \"24 Cards\"";

    // This is the known-library story's pile, with the two known cards on top.
    let mut view = ViewBuilder::new();
    for _ in 0..22 {
        view.card(ZoneName::Library, Side::You, "");
    }
    for name in ["No More Lies", "Abhorrent Oculus"] {
        view.card(ZoneName::Library, Side::You, name).name = Some(name.to_owned());
    }
    view.card(ZoneName::Graveyard, Side::You, "Island");
    view.script(ScriptBuilder::reset().build());
    let update = FixtureMessage::game_update(&view.update());
    let cards = fixture_message(std::slice::from_ref(&update), TokenMakers::NONE);
    let data = crate::test_data::data();
    let size = Size {
        width: 1728.0,
        height: 1117.0,
    };
    let mut page = Page::new(
        Arc::clone(&data),
        Recording(vec![cards, update]).into(),
        Side::You,
        size,
    )
    .expect("the view can be read")
    .with_host_atlas()
    .with_scale(SCALE);
    page.set_looking(true);
    page.still();
    assert!(page.try_find(TITLE).is_err());
    assert!(page.try_find(COUNT).is_err());

    let mut renderer = Renderer::headless(Limits::Default, data.lighting).expect("a GPU device");
    assert!(renderer.upload_page(&light::prepared(&data.light, &data.own)));
    let (send, received) = mpsc::channel();
    let worker = spawn_glyph_worker(send, || {}, Some(&data.profiles));
    let mut deliver = |page: &mut Page| {
        worker.flush();
        let mut highest = 0;
        for prepared in received.try_iter() {
            let Upload::AtlasPage { page: layer, .. } = &prepared.upload else {
                panic!("the glyph worker sends atlas pages");
            };
            if renderer.upload_page(&prepared) {
                highest = highest.max(*layer);
                page.give_glyphs(prepared.ready);
            }
        }
        highest
    };
    worker.icons(SCALE);
    deliver(&mut page);

    // Large capitals fill the original pages without preloading the viewer's text sizes.
    let font = FontRef::from_index(Font::OpenSans.file().data, 0).unwrap();
    let charmap = font.charmap();
    worker.request([400, 500, 600].into_iter().flat_map(|size| {
        ('A'..='Z').map(move |character| GlyphKey {
            blur: 0,
            font: Font::OpenSans as u8,
            weight: 400,
            glyph: charmap.map(character),
            size,
        })
    }));
    let prefilled = deliver(&mut page);
    worker.request(page.wanted_glyphs());
    deliver(&mut page);

    page.press("pile \"Your library\"");
    page.still();
    worker.request(page.wanted_glyphs());
    deliver(&mut page);

    let labels = [page.find(TITLE), page.find(COUNT)];
    let frame = page.frame();
    for label in labels {
        assert!(
            !frame.undrawn.iter().any(|part| part.element == label.index),
            "{} is missing glyphs after atlas delivery",
            label.name
        );
        assert!(
            frame.draws.iter().zip(&frame.owners).any(|(draw, &owner)| {
                owner == label.index
                    && draw.material == Material::Glyph as u8
                    && draw.layer >= ORIGINAL_ATLAS_PAGES
            }),
            "{} draws glyphs on the grown atlas",
            label.name
        );
    }
    assert!(
        prefilled >= ORIGINAL_ATLAS_PAGES,
        "atlas reached page {prefilled} before opening"
    );
    let mut target = Target::offscreen(&renderer, 3456, 2234);
    renderer.render(frame, &mut target);
}
