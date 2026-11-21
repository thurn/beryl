//! The contract's types hold their promised shapes.

#![expect(
    clippy::float_cmp,
    reason = "the constructors copy their arguments, so the values are exact"
)]

use app_client::Input;
use app_client::command::Reply;
use app_client::{Client, Config};
use glam::{Affine3A, Vec2, Vec3};
use kit_frame::frame::{Draw, Frame, Material, Sprite, Wake};
use kit_ui::element::Shadow;
use kit_ui::input::{
    Button, FaceSize, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size,
};

#[test]
fn a_draw_record_is_80_bytes() {
    assert_eq!(size_of::<Draw>(), 80);
}

const TINT: [u8; 4] = [10, 20, 30, 40];
const SPRITE: Sprite = Sprite {
    page: 3,
    rect: [1, 2, 3, 4],
};

fn transform() -> Affine3A {
    Affine3A::from_translation(Vec3::new(1.0, 2.0, 3.0))
}

/// Each constructor writes the given material, the transform and the tint, and the `uv`,
/// `layer` and `params` the doc's table gives that material, with every other field at zero.
#[test]
fn each_constructor_fills_only_its_materials_fields() {
    let cases = [
        (
            Draw::table(transform(), TINT),
            Material::Table,
            [0; 4],
            0,
            [0.0; 4],
        ),
        (
            Draw::tabletop(transform(), TINT),
            Material::Table,
            [0; 4],
            0,
            [1.0, 0.0, 0.0, 0.0],
        ),
        (
            Draw::card(transform(), TINT, FaceSize::Board, 7, 9, 0.25, 0.5),
            Material::Card,
            [9, 0, 0, 0],
            7,
            [0.25, 0.5, 0.0, 0.0],
        ),
        (
            Draw::contact_shadow(transform(), TINT, 4.0),
            Material::ContactShadow,
            [0; 4],
            0,
            [4.0, 0.0, 0.0, 0.0],
        ),
        (
            Draw::glow(transform(), TINT, SPRITE, 0.5),
            Material::Glow,
            SPRITE.rect,
            3,
            [0.5, 0.0, 0.0, 0.0],
        ),
        (
            Draw::streak(transform(), TINT, SPRITE, 0.5, 0.25),
            Material::Streak,
            SPRITE.rect,
            3,
            [0.5, 0.25, 0.0, 0.0],
        ),
        (
            Draw::panel(
                transform(),
                TINT,
                [255, 1, 2, 3],
                Vec2::new(100.0, 40.0),
                6.0,
                1.0,
            ),
            Material::Panel,
            [255, 1, 2, 3],
            0,
            [100.0, 40.0, 6.0, 1.0],
        ),
        (
            Draw::glyph(transform(), TINT, SPRITE),
            Material::Glyph,
            SPRITE.rect,
            3,
            [0.0; 4],
        ),
        (
            Draw::icon(transform(), TINT, SPRITE),
            Material::Icon,
            SPRITE.rect,
            3,
            [0.0; 4],
        ),
        (
            Draw::particle(transform(), TINT, SPRITE, 0.75, 11.0),
            Material::Particle,
            SPRITE.rect,
            3,
            [0.75, 11.0, 0.0, 0.0],
        ),
    ];
    for (draw, material, uv, layer, params) in cases {
        assert_eq!(draw.material, material as u8, "{material:?}");
        assert_eq!(draw.uv, uv, "{material:?}");
        assert_eq!(draw.layer, layer, "{material:?}");
        assert_eq!(draw.params, params, "{material:?}");
        assert_eq!(draw.tint, TINT, "{material:?}");
        assert_eq!(draw.clip(), 0, "{material:?}");
        assert_eq!(
            draw.model,
            [1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 2.0, 0.0, 0.0, 1.0, 3.0],
            "{material:?}"
        );
    }
}

/// A box shadow keeps its lengths in `uv` as sixteenths of a point plus 32,768, its inset in
/// `layer`, and its box and deviation in `params`.
#[test]
fn a_box_shadow_fills_only_its_fields() {
    let draw = Draw::box_shadow(
        transform(),
        TINT,
        Vec2::new(100.0, 40.0),
        8.0,
        &Shadow::new(-1.0, 6.0, 16.0, [0, 0, 0, 128])
            .spread(2.0)
            .inset(),
        8.5,
    );
    assert_eq!(draw.material, Material::BoxShadow as u8);
    // -1, 6, 2 and 8.5 points.
    assert_eq!(draw.uv, [32_752, 32_864, 32_800, 32_904]);
    assert_eq!(draw.layer, 1);
    // A blur of 16 points is a deviation of 8 by CSS, 6.8 as WebKit draws it.
    assert_eq!(draw.params, [100.0, 40.0, 8.0, 6.8]);
    assert_eq!(draw.tint, TINT);
}

#[test]
fn a_clip_keeps_the_texture_array_and_mesh() {
    let draw = Draw::glyph(transform(), TINT, SPRITE);
    let clipped = draw.with_clip(5);
    assert_eq!(clipped.clip(), 5);
    assert_eq!(clipped.flags & 0x0f, draw.flags & 0x0f);
}

fn inputs() -> Vec<Input> {
    vec![
        Input::Surface {
            size: Size {
                width: 1280.0,
                height: 800.0,
            },
            scale: 2.0,
            safe_area: Insets::default(),
        },
        Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase: PointerPhase::Move,
            position: Vec2::new(300.0, 200.0),
            button: Button::None,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }),
        Input::Reply(Reply::Done(Ok(()))),
    ]
}

fn play() -> Vec<Frame> {
    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 64,
            draws: 64,
            name_bytes: 1024,
        },
    );
    let mut frames = Vec::new();
    for (step, input) in inputs().into_iter().enumerate() {
        client.handle(input);
        assert_eq!(client.wake(), Wake::Now);
        let now = 16_666_667 * u64::try_from(step).unwrap();
        frames.push(client.frame(now).clone());
        assert_eq!(client.wake(), Wake::Never);
    }
    frames
}

#[test]
fn the_same_inputs_and_times_give_the_same_frames() {
    assert_eq!(play(), play());
}

mod milestone_three {
    use app_client::command::{Request, SessionCommand};
    use beryl_protocol::prompt::{ButtonId, Click, ClickRequest, PromptView, ShapeBody};
    use beryl_protocol::protocol::{LossReason, OutcomeView, Side, Status};
    use beryl_protocol::script::Arrow;
    use game_hud::prompts::bar::Message as BarMessage;
    use kit_ui::element::Action;

    use app_client::settings::{Setting, Settings};
    use app_screens::screens::settings::{
        Inputs, Message as SettingsMessage, SettingsSheet, State,
    };
    use app_screens::screens::{self, Music, Screen, ScreenIn, bug_report, start, testing};
    use beryl_protocol::testing::ViewBuilder;
    use game_hud::prompts;
    use game_hud::screens::game::Furniture;
    use game_table::table::Window;
    use kit_frame::frame::Role;
    use kit_scene::Feature;
    use kit_ui::widgets::{FieldState, Fields};

    static TUNING: std::sync::OnceLock<std::sync::Arc<kit_base::data::Data>> =
        std::sync::OnceLock::new();

    static FIELDS: Fields = Fields::new();

    static TABLE: game_manual::manual::TableState = game_manual::manual::TableState::new();
    static ENTRIES: screens::TableEntries = screens::TableEntries::new();
    static LOBBY: game_manual::manual::lobby::Lobby = game_manual::manual::lobby::Lobby::new();

    use super::super::flat_layer::{client, deciding, decision, find, prompt};

    fn input<'a>(prompt: Option<&'a PromptView>, settings: &'a Settings) -> ScreenIn<'a> {
        ScreenIn {
            screen: Screen::Game,
            window: Window::default(),
            profile: game_table::profiles::Profile::for_window(
                &TUNING.get_or_init(crate::test_data::data).profiles,
                Window::default(),
            ),
            view: None,
            played: None,
            turn: None,
            prompt,
            full_control: settings.full_control,
            stops: settings.stops,
            bindings: &settings.bindings,
            sheet: None,
            spinning: false,
            gallery: screens::GalleryState::default(),
            fields: &FIELDS,
            inspected: None,
            faces: None,
            viewing: None,
            revealed: &[],
            table: &TABLE,
            carried: None,
            lobby: &LOBBY,
            placement_scene: None,
        }
    }

    #[test]
    fn every_screen_and_sheet_gives_a_named_element() {
        let settings = Settings::default();
        let input = input(None, &settings);
        for screen in Screen::ALL {
            let input = ScreenIn { screen, ..input };
            let element = match screen {
                Screen::Start => start::StartScreen::view(
                    &start::State::default(),
                    start::Inputs {
                        window: input.window,
                        column: input.profile.screen_column(),
                        in_progress: false,
                        session: None,
                        has_recordings: false,
                        entries: &ENTRIES,
                        fields: &FIELDS,
                    },
                ),
                Screen::Testing => testing::TestingScreen::view(
                    &testing::State::default(),
                    testing::Inputs {
                        window: input.window,
                        column: input.profile.screen_column(),
                        filter: FieldState::default(),
                    },
                ),
                _ => screens::screen(&input, Furniture::default(), None, None)
                    .expect("the screen has a view"),
            };
            assert!(!element.name.is_empty(), "{screen:?}");
        }
        assert!(!bug_report::compose(None, None).name.is_empty());
        let settings = SettingsSheet::view(
            &State::default(),
            Inputs {
                window: input.window,
                screen: input.screen,
                settings: settings.screen(),
                game_over: false,
                layout: app_screens::screens::settings::Layout::new(
                    game_table::profiles::Profile::for_window(
                        &crate::test_data::data().profiles,
                        input.window,
                    ),
                ),
            },
        );
        assert!(!settings.name.is_empty());
    }

    /// One prompt body of each shape.
    fn bodies() -> Vec<ShapeBody> {
        vec![
            ShapeBody::Pick {
                tray: Vec::new(),
                least: 1,
                most: 1,
                picked: Vec::new(),
                warned: Vec::new(),
            },
            ShapeBody::Order { items: Vec::new() },
            ShapeBody::Split { piles: Vec::new() },
            ShapeBody::YesNo {
                about: Vec::new(),
                places: None,
            },
            ShapeBody::Number {
                values: vec![0, 1],
                value: 0,
                detail: None,
            },
            ShapeBody::Mode { about: Vec::new() },
            ShapeBody::Catalog {
                entries: vec!["Plains".to_owned()],
                chosen: None,
            },
            ShapeBody::Divide {
                total: 3,
                left: 3,
                shares: Vec::new(),
            },
            ShapeBody::Pay {
                cost: "{1}".to_owned(),
                left: "{1}".to_owned(),
            },
            ShapeBody::Declare {
                arrow: Arrow::Attack,
                pairs: Vec::new(),
                lined: false,
                aiming: Vec::new(),
            },
            ShapeBody::Candidates,
        ]
    }

    #[test]
    fn every_prompt_shape_shows_the_prompt_bar() {
        for body in bodies() {
            let mut client = client();
            let mut shaped = prompt("Choose");
            shaped.body = body;
            let title = prompts::prompt_title(&shaped, false).into_owned();
            client.handle(deciding(shaped));
            let frame = client.frame(0);
            find(frame, Role::Text, &title);
            find(frame, Role::Button, "Auto Pay");
            find(frame, Role::Button, "Cancel");
        }
    }

    #[test]
    fn a_button_press_clicks_the_open_prompt() {
        let mut client = client();
        client.handle(deciding(prompt("Pay {1}{U}")));
        client.press(&Action::new(BarMessage::Button(ButtonId::Auto)));
        assert_eq!(
            client.next_command(),
            Some(SessionCommand::Click(ClickRequest {
                decision: decision(),
                revision: 3,
                click: Click::Button {
                    button: ButtonId::Auto
                },
            }))
        );
    }

    #[test]
    fn a_setting_press_asks_for_the_settings_to_be_saved() {
        let mut client = client();
        while client.next_request().is_some() {}
        client.press(&Action::new(SettingsMessage::Change(Setting::Music(40))));
        let Some(Request::SaveSettings(saved)) = client.next_request() else {
            panic!("the first request saves the settings");
        };
        assert_eq!(saved.music, 40);
    }

    #[test]
    fn the_graphics_setting_is_saved_and_starts_automatic() {
        use app_client::settings::Graphics;
        let mut settings = Settings::default();
        assert_eq!(settings.graphics, Graphics::Automatic);
        settings.apply(Setting::Graphics(Graphics::Baseline));
        assert_eq!(settings.graphics, Graphics::Baseline);
        let mut client = client();
        while client.next_request().is_some() {}
        client.press(&Action::new(SettingsMessage::Change(Setting::Graphics(
            Graphics::Enhanced,
        ))));
        let Some(Request::SaveSettings(saved)) = client.next_request() else {
            panic!("the first request saves the settings");
        };
        assert_eq!(saved.graphics, Graphics::Enhanced);
    }

    #[test]
    fn a_finished_game_the_viewer_won_plays_the_win() {
        let mut view = ViewBuilder::sample().build();
        view.status = Status::Over {
            outcome: OutcomeView::Win {
                winner: Side::You,
                reason: LossReason::Life,
                by: None,
            },
        };
        assert_eq!(screens::music(Screen::Game, Some(&view)), Some(Music::Win));
        view.status = Status::Thinking;
        assert_eq!(screens::music(Screen::Game, Some(&view)), Some(Music::Game));
        assert_eq!(screens::music(Screen::Gallery, Some(&view)), None);
    }
}
