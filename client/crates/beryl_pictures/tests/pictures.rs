//! `client png` and `client film`, run as an agent runs them: the picture shows the card that the
//! tree says is in flight, a film is one picture of bounded width with a line for each frame,
//! and two runs started together write two files, and the headless host draws the scans in its
//! scan cache, prepared off its own thread. They need a GPU adapter (Metal on a Mac).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Output};

use beryl_assets::faces_built_on_this_thread;
use beryl_assets::glyphs::glyphs_rasterized_on_this_thread;
use beryl_assets::icons::icons_rasterized_on_this_thread;
use beryl_host::{Headless, Scans, match_faces};
use beryl_inspect::pages;
use beryl_protocol::protocol::Side;
use game_probe::driver::Moment;
use kit_base::data::profiles::SizeClass;
use kit_base::tokens::shadow;
use kit_frame::frame::{StateBits, box_shadow_reach};
use kit_scene::hit::{self, uncovered};
use kit_ui::input::Size;

const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

fn render(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_client-render"))
        .args(args)
        .output()
        .expect("the program starts")
}

/// The file a successful run says it wrote, and its size from the PNG header.
fn written(output: &Output) -> (PathBuf, (u32, u32)) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "failed: {}\n{stdout}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix("wrote "))
        .unwrap_or_else(|| panic!("no file named in:\n{stdout}"));
    let path = PathBuf::from(line.split(" (").next().expect("a path"));
    let bytes = std::fs::read(&path).expect("the file exists");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let number = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
    (path, (number(16), number(20)))
}

#[test]
fn a_press_reaches_the_card_that_paints_the_pixel() {
    use beryl_render::{Limits, Renderer, Target};
    use glam::{Vec2, camera::rh::proj::directx};
    use kit_base::atlas::Tier;
    use kit_frame::frame::{ElementId, Frame, FrameBuilder};

    let card = Vec2::new(63.0, 88.0);
    let projection = directx::perspective(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 10.0);
    let lighting = beryl_inspect::data::checkout().expect("tuning").lighting;
    let mut renderer = Renderer::headless(Limits::Default, lighting).expect("a GPU");
    let format = beryl_assets::Format::choose(&renderer.formats());
    renderer.set_faces(&["", "Island", "Plains"], format);
    let mut target = Target::offscreen(&renderer, 64, 64);
    for (tier, state) in [
        (Tier::Baseline, 0),
        (Tier::Baseline, StateBits::MOVING),
        (Tier::Enhanced, 0),
        (Tier::Enhanced, StateBits::MOVING),
    ] {
        for (angle, red_alpha, expected) in [
            (0.0, 255, [1, 1]),  // Coplanar opaque cards, later registration wins.
            (0.45, 255, [0, 1]), // Crossing opaque cards, nearer at each point wins.
            (0.0, 224, [0, 0]),  // The blended card paints over equal opaque depth.
            (0.45, 224, [0, 1]), // Opaque depth hides the blended card on one side only.
        ] {
            let mut frame = Frame::with_capacity(2, 2, 64);
            let mut builder = FrameBuilder::new(&mut frame, tier, card);
            builder.camera(projection);
            crossing_cards(&mut builder, projection, state, angle, red_alpha);
            let frame = builder.finish();
            renderer.render(frame, &mut target);
            let pixels = renderer.read(&target);
            for (x, expected) in [24, 39].into_iter().zip(expected) {
                let point = Vec2::new(x as f32 + 0.5, 32.5);
                let at = (32 * 64 + x) * 4;
                let pixel = &pixels[at..at + 4];
                let painted = usize::from(pixel[2] > pixel[0]);
                assert_ne!(pixel[0], pixel[2], "a card paints {point}");
                assert_eq!(
                    painted, expected,
                    "{tier:?} state {state}, angle {angle}, alpha {red_alpha}: pixel at {point}"
                );
                let hit = hit::hit(frame, point).expect("a card at the press") as usize;
                assert_eq!(
                    frame.resolved[hit].id,
                    ElementId(painted as u32),
                    "{tier:?} state {state}, angle {angle}, alpha {red_alpha}: pick at {point}"
                );
            }
        }
    }
}

fn crossing_cards(
    builder: &mut kit_frame::frame::FrameBuilder<'_>,
    projection: glam::Mat4,
    state: u32,
    angle: f32,
    red_alpha: u8,
) {
    use beryl_render::BACK_LAYER;
    use glam::{Quat, Vec2, Vec3};
    use kit_base::{atlas::FaceSize, pose::Pose};
    use kit_frame::frame::{Band, Draw, ElementId, NameRef, Place, Resolved, Role};

    let card = builder.card();
    let cards = [
        ("Island", [255, 0, 0, red_alpha]),
        ("Plains", [0, 0, 255, 255]),
    ];
    for (index, (name, tint)) in cards.into_iter().enumerate() {
        let pose = Pose {
            position: Vec3::NEG_Z * 2.0,
            rotation: Quat::from_rotation_y(if index == 0 { angle } else { -angle }),
            scale: 2.0 / card.y,
            opacity: f32::from(tint[3]) / 255.0,
            ..Pose::default()
        };
        let transform = pose.transform();
        let half = card * 0.5;
        let corners = [
            (-half.x, -half.y),
            (half.x, -half.y),
            (half.x, half.y),
            (-half.x, half.y),
        ]
        .map(|(x, y)| {
            let projected =
                projection.project_point3(transform.transform_point3(Vec3::new(x, y, 0.0)));
            (Vec2::ONE + projected.truncate() * Vec2::new(1.0, -1.0)) * 32.0
        });
        let element = builder.resolved(
            name,
            Resolved {
                id: ElementId(index as u32),
                parent: u32::MAX,
                role: Role::Card,
                name: NameRef::default(),
                place: Place::named("preview"),
                pose: Some(pose),
                corners,
                depth: 2.0,
                opacity: pose.opacity,
                clip: None,
                interactive: true,
                state: StateBits(state),
                face: None,
                count: 1,
                annotations: Default::default(),
            },
        );
        let resting = if tint[3] == 255 {
            Band::Opaque
        } else {
            Band::RaisedMarks
        };
        builder.draw_card(
            element,
            resting,
            Draw::card(
                transform,
                tint,
                FaceSize::Board,
                index as u16 + 1,
                BACK_LAYER,
                0.0,
                0.0,
            ),
        );
    }
}

#[test]
fn the_picture_shows_the_card_the_tree_says_is_in_flight() {
    let page = pages::page("verbs-zones", Side::You, SizeClass::Regular, WINDOW).expect("a page");
    let mut head = Headless::new(page, WINDOW.width, WINDOW.height, &Scans::app()).expect("a GPU");
    head.seek(&"1500ms".parse::<Moment>().expect("a moment"))
        .expect("a moment of the recording");
    let frame = head.frame();
    let flying = (0..frame.resolved.len() as u32)
        .find(|&index| {
            let element = &frame.resolved[index as usize];
            element.state.has(StateBits::MOVING) && uncovered(frame, index).is_some()
        })
        .expect("a card is in flight at 1500 ms");
    let inside = uncovered(frame, flying).expect("part of it is uncovered");
    let (x, y) = (
        (inside.x + inside.width / 2.0) as u32,
        (inside.y + inside.height / 2.0) as u32,
    );
    let pixels = head.pixels();
    let pixel = |x: u32, y: u32| {
        let at = ((y * WINDOW.width as u32 + x) * 4) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    };
    // The table is most of the picture, so its colour is the most common one.
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for y in 0..WINDOW.height as u32 {
        for x in 0..WINDOW.width as u32 {
            *counts.entry(pixel(x, y)).or_default() += 1;
        }
    }
    let table = counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .expect("pixels")
        .0;
    assert_ne!(
        pixel(x, y),
        table,
        "the card at ({x}, {y}) is the table's colour"
    );
}

/// A directory of its own for one test, emptied first and removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("beryl_pictures-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cards_show_the_scans_in_the_cache_prepared_on_other_threads() {
    let scratch = Scratch::new("scans");
    let scans = Scans {
        art: scratch.0.join("art"),
        faces: scratch.0.join("faces"),
    };
    let page = pages::page("verbs-zones", Side::You, SizeClass::Regular, WINDOW).expect("a page");
    // Every face's scan, the card back's included, is one flat magenta picture.
    let magenta = [230, 20, 200];
    let (width, height) = (672, 936);
    let pixels: Vec<u8> = (0..width * height)
        .flat_map(|_| [magenta[0], magenta[1], magenta[2], 255])
        .collect();
    // Written as a PNG, which the decoder reads whatever the file is called.
    let scan = scratch.0.join("scan.png");
    beryl_render::write_rgba_png(&scan, &pixels, width, height).expect("a scan");
    let faces = match_faces(page.faces());
    assert!(faces.faces.len() > 1, "the recording lists its faces");
    for face in &faces.faces {
        let file = scans.art.join(&face.scan[1..]);
        std::fs::create_dir_all(file.parent().expect("a directory")).expect("a directory");
        std::fs::copy(&scan, &file).expect("a scan in the cache");
    }
    let mut head = Headless::new(page, WINDOW.width, WINDOW.height, &scans).expect("a GPU");
    assert_eq!(
        faces_built_on_this_thread(),
        0,
        "a face was built on the drawing thread"
    );
    head.seek(&"start".parse::<Moment>().expect("a moment"))
        .expect("a moment of the recording");
    let frame = head.frame();
    let card = (0..frame.resolved.len() as u32)
        .find_map(|index| {
            let rect = uncovered(frame, index)?;
            (rect.width >= 8.0 && rect.height >= 8.0).then_some(rect)
        })
        .expect("a card shows at the start");
    let (x, y) = (
        (card.x + card.width / 2.0) as u32,
        (card.y + card.height / 2.0) as u32,
    );
    let pixels = head.pixels();
    let at = ((y * WINDOW.width as u32 + x) * 4) as usize;
    let found = [pixels[at], pixels[at + 1], pixels[at + 2]];
    assert!(
        found.iter().zip(magenta).all(|(&a, b)| a.abs_diff(b) <= 12),
        "the card at ({x}, {y}) is {found:?}, not its scan's {magenta:?}"
    );
}

#[test]
fn the_prompt_bar_draws_text_rasterized_on_another_thread() {
    let page = pages::page("pay-default", Side::You, SizeClass::Regular, WINDOW).expect("a page");
    let mut head = Headless::new(page, WINDOW.width, WINDOW.height, &Scans::app()).expect("a GPU");
    head.seek(&"prompt:1".parse::<Moment>().expect("a moment"))
        .expect("a moment of the recording");
    let button = head.page().find(r#"button "Next""#).rect();
    let pixels = head.pixels();
    assert_eq!(
        glyphs_rasterized_on_this_thread(),
        0,
        "a glyph was rasterized on the drawing thread"
    );
    assert_eq!(
        icons_rasterized_on_this_thread(),
        0,
        "an icon was rasterized on the drawing thread"
    );
    // The button is dark with a dull border; only its label's ink is near white.
    let inked = (button.y as u32..(button.y + button.height) as u32)
        .flat_map(|y| (button.x as u32..(button.x + button.width) as u32).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let at = ((y * WINDOW.width as u32 + x) * 4) as usize;
            pixels[at..at + 3].iter().all(|&channel| channel > 200)
        })
        .count();
    assert!(
        inked > 20,
        "the button's label drew {inked} near-white pixels"
    );
}

#[test]
fn an_element_drawn_alone_leaves_the_rest_of_the_window_black() {
    let page = pages::page("pay-default", Side::You, SizeClass::Regular, WINDOW).expect("a page");
    let mut head = Headless::new(page, WINDOW.width, WINDOW.height, &Scans::app()).expect("a GPU");
    // Only a looking frame knows which element each record belongs to.
    head.page().set_looking(true);
    head.seek(&"prompt:1".parse::<Moment>().expect("a moment"))
        .expect("a moment of the recording");
    let button = head.page().find(r#"button "Next""#);
    let (index, rect) = (button.index, button.rect());
    let pixels = head.pixels_keeping(|frame, at| at == index || hit::is_within(frame, at, index));
    // A point of slack around the box, for the edge's antialiasing, and as far as the primary
    // button's box shadows reach past it.
    let reach = (shadow::PRIMARY.iter())
        .map(|shadow| box_shadow_reach(shadow) + shadow.x.abs().max(shadow.y.abs()))
        .fold(0.0_f32, f32::max);
    let slack = 1.0 + reach;
    let inside = |x: u32, y: u32| {
        let (x, y) = (x as f32, y as f32);
        x + slack >= rect.x
            && y + slack >= rect.y
            && x <= rect.x + rect.width + slack
            && y <= rect.y + rect.height + slack
    };
    let (mut lit_inside, mut lit_outside) = (0, 0);
    for y in 0..WINDOW.height as u32 {
        for x in 0..WINDOW.width as u32 {
            let at = ((y * WINDOW.width as u32 + x) * 4) as usize;
            if pixels[at..at + 3].iter().any(|&channel| channel > 0) {
                if inside(x, y) {
                    lit_inside += 1;
                } else {
                    lit_outside += 1;
                }
            }
        }
    }
    assert_eq!(lit_outside, 0, "pixels drawn outside the button");
    assert!(lit_inside > 100, "the button drew {lit_inside} pixels");
}

#[test]
fn a_film_is_one_narrow_picture_with_a_line_for_each_frame() {
    let output = render(&[
        "film",
        "verbs-zones",
        "--from",
        "0ms",
        "--to",
        "3s",
        "--frames",
        "12",
    ]);
    let (path, (width, height)) = written(&output);
    assert!(width <= 2000 && height > 0, "{width}x{height}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout
        .lines()
        .filter(|line| line.starts_with("  #"))
        .count();
    assert_eq!(lines, 12, "{stdout}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn two_runs_started_together_write_two_files() {
    let args = ["png", "verbs-zones", "--at", "1500ms"];
    let first = Command::new(env!("CARGO_BIN_EXE_client-render"))
        .args(args)
        .spawn_output();
    let second = render(&args);
    let (a, _) = written(&first());
    let (b, _) = written(&second);
    assert_ne!(a, b);
    let _ = std::fs::remove_file(a);
    let _ = std::fs::remove_file(b);
}

#[test]
fn a_film_too_long_for_its_grid_is_refused_with_what_would_work() {
    let output = render(&["film", "verbs-zones", "--frames", "30"]);
    assert_eq!(output.status.code(), Some(2));
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("30 frames") && message.contains("--frames 24"),
        "{message}"
    );
}

/// Starts a command and returns a function that waits for its output.
trait SpawnOutput {
    fn spawn_output(&mut self) -> Box<dyn FnOnce() -> Output>;
}

impl SpawnOutput for Command {
    fn spawn_output(&mut self) -> Box<dyn FnOnce() -> Output> {
        let child = self
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the program starts");
        Box::new(move || child.wait_with_output().expect("it finishes"))
    }
}

/// At scale 2 the client gets the scale, so its clips are in physical pixels like the target:
/// the start screen's opponent list, which sits inside a clip, draws its rows. When the test
/// driver told the client scale 1 and the host only made its target twice as large, the clip
/// stayed in scale-1 pixels and the list was drawn empty.
#[test]
fn a_picture_at_scale_2_draws_what_its_clips_hold() {
    let scale = 2.0;
    let page = pages::page("screen/start", Side::You, SizeClass::Regular, WINDOW)
        .expect("a page")
        .with_scale(scale);
    let mut head = Headless::new(page, WINDOW.width, WINDOW.height, &Scans::app()).expect("a GPU");
    assert_eq!(head.size(), (3200, 1800));
    head.seek(&"end".parse::<Moment>().expect("a moment"))
        .expect("a moment of the recording");
    let list = head.page().find(r#"list "Opponent""#).rect();
    let pixels = head.pixels();
    let width = head.size().0;
    let (left, top) = ((list.x * scale) as u32, (list.y * scale) as u32);
    let (right, bottom) = (
        ((list.x + list.width) * scale) as u32,
        ((list.y + list.height) * scale) as u32,
    );
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for y in top..bottom {
        for x in left..right {
            let at = ((y * width + x) * 4) as usize;
            *counts
                .entry([pixels[at], pixels[at + 1], pixels[at + 2]])
                .or_default() += 1;
        }
    }
    let total: u32 = counts.values().sum();
    let background = counts.values().max().copied().unwrap_or(0);
    // Three rows of text, radio buttons and a selected row's fill cover far more than this.
    assert!(
        total - background > total / 10,
        "the opponent list at {list:?} drew {} of {total} pixels off its background",
        total - background
    );
}

#[path = "pictures/downlevel.rs"]
mod downlevel;
#[path = "pictures/enhanced_boards.rs"]
mod enhanced_boards;
#[path = "pictures/light.rs"]
mod light;
