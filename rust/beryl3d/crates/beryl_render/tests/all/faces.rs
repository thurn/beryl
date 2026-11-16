//! Card faces in the renderer: a face compressed by `beryl_assets` draws its picture, every
//! format's generated placeholder draws its colors, and faces that arrive during play are
//! uploaded only as fast as the client's animations allow. Every test runs on a device with
//! wgpu's default limits and on one with its downlevel limits.

use beryl_assets::{Format, Layer, MatchFace, Prepared, Source, Upload, dimensions, encode, frame};
use beryl_render::{BACK_LAYER, Limits, Renderer, Target};
use beryl_scene::faces::PrintedText;
use beryl_scene::frame::{Band, Draw, Frame, FrameBuilder, Wake};
use beryl_scene::input::{AssetReady, FaceId, FaceSize};
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Affine3A, Vec3};

const BOTH: [Limits; 2] = [Limits::Default, Limits::Downlevel];

/// The formats to try on `renderer`: each compressed one it samples, and uncompressed.
fn formats(renderer: &Renderer) -> Vec<Format> {
    let mut formats = renderer.formats();
    formats.push(Format::Rgba8);
    formats
}

/// Half the width of the table the camera shows, in millimetres: `FAR` shows the whole card,
/// 63 by 88 millimetres, in about 63 pixels, so a face is drawn from a smaller mipmap level, and
/// `NEAR` shows its middle at about the face's own size, drawn from the largest level.
const FAR: f32 = 100.0;
const NEAR: f32 = 32.0;

/// Draws one card lying face up in the middle of a 200 by 200 pixel target, its front showing
/// board-size layer `face`, with the camera looking straight down on the square of the table
/// from -`half` to `half` millimetres.
fn draw_card(renderer: &mut Renderer, face: u16, half: f32) -> Picture {
    let camera = directx::orthographic(-half, half, -half, half, 0.0, 2000.0)
        * look_at_mat4(Vec3::new(0.0, 0.0, 1000.0), Vec3::ZERO, Vec3::Y);
    let card = Draw::card(
        Affine3A::from_translation(Vec3::new(0.0, 0.0, 1.0)),
        [255; 4],
        FaceSize::Board,
        face,
        BACK_LAYER,
        0.0,
        0.0,
    );
    let mut frame = Frame::with_capacity(0, 1, 0);
    let mut builder = FrameBuilder::new(&mut frame);
    builder.camera(camera);
    builder.draw(Band::Opaque, card);
    let frame = builder.finish();
    let mut target = Target::offscreen(renderer, 200, 200);
    renderer.render(frame, &mut target);
    Picture {
        pixels: renderer.read(&target),
        half,
    }
}

struct Picture {
    pixels: Vec<u8>,
    half: f32,
}

/// The color drawn at the table's point (x, y), in millimetres.
fn at(picture: &Picture, x: f32, y: f32) -> [u8; 3] {
    let pixel = |mm: f32| ((mm + picture.half) / (2.0 * picture.half) * 200.0) as usize;
    let i = ((199 - pixel(y)) * 200 + pixel(x)) * 4;
    let p = &picture.pixels;
    [p[i], p[i + 1], p[i + 2]]
}

fn assert_near(found: [u8; 3], expected: [u8; 3], tolerance: u8, what: &str) {
    assert!(
        found
            .iter()
            .zip(expected)
            .all(|(&a, b)| a.abs_diff(b) <= tolerance),
        "{what}: found {found:?}, expected {expected:?} within {tolerance}"
    );
}

/// A board-size face, left half `left` and right half `right`, compressed in `format` as the
/// asset workers would send it for `face`.
fn two_color_face(face: u16, format: Format, left: [u8; 3], right: [u8; 3]) -> Prepared {
    let (width, height) = dimensions(FaceSize::Board);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for x in 0..width {
            let [r, g, b] = if x < width / 2 { left } else { right };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let layer = encode(&pixels, FaceSize::Board, format).expect("the face compresses");
    prepared(face, FaceSize::Board, face, layer)
}

fn prepared(face: u16, size: FaceSize, index: u16, layer: Layer) -> Prepared {
    Prepared {
        upload: Upload::Face {
            layer,
            source: Source::Encoded,
        },
        ready: AssetReady::Face {
            face: FaceId(face),
            size,
            layer: index,
        },
    }
}

#[test]
fn a_compressed_face_draws_its_picture() {
    let left = [200, 40, 40];
    let right = [40, 90, 210];
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        for format in formats(&renderer) {
            renderer.set_faces(&["", "Island"], format);
            assert!(
                renderer.upload_layer(&two_color_face(1, format, left, right)),
                "{limits:?} {format:?}: the face was not uploaded"
            );
            let picture = draw_card(&mut renderer, 1, FAR);
            let what = format!("{limits:?} {format:?}");
            // etcpak puts a flat color up to 8 away.
            assert_near(at(&picture, -15.0, 0.0), left, 10, &what);
            assert_near(at(&picture, 15.0, 0.0), right, 10, &what);
        }
    }
}

#[test]
fn every_format_draws_the_same_placeholders() {
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        renderer.set_faces(&["", "Island"], Format::Rgba8);
        let face = draw_card(&mut renderer, 1, NEAR);
        let back = draw_card(&mut renderer, 0, NEAR);
        assert_ne!(at(&face, 0.0, 0.0), at(&back, 0.0, 0.0), "{limits:?}");
        for format in renderer.formats() {
            renderer.set_faces(&["", "Island"], format);
            let what = format!("{limits:?} {format:?}");
            // ETC2's solid blocks keep 4 bits a channel.
            let compressed_face = draw_card(&mut renderer, 1, NEAR);
            let compressed_back = draw_card(&mut renderer, 0, NEAR);
            for x in [-25.0, -9.0, 0.0, 13.0, 27.0] {
                for y in [-28.0, -10.0, 0.0, 6.0, 22.0] {
                    let what = format!("{what} at ({x}, {y})");
                    assert_near(at(&compressed_face, x, y), at(&face, x, y), 12, &what);
                    assert_near(at(&compressed_back, x, y), at(&back, x, y), 12, &what);
                }
            }
        }
    }
}

#[test]
fn a_drawn_island_shows_its_blue_frame_s_pale_name_bar() {
    let island = MatchFace {
        scan: String::new(),
        key: String::new(),
        names: vec!["Island".to_owned()],
        colors: Vec::new(),
        text: PrintedText {
            type_line: "Basic Land — Island".to_owned(),
            rules: Some("({T}: Add {U}.)".to_owned()),
            ..PrintedText::default()
        },
        plate: None,
    };
    let pixels = frame(&island, FaceSize::Board);
    let layer = encode(&pixels, FaceSize::Board, Format::Rgba8).expect("the face compresses");
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        renderer.set_faces(&["", "Island"], Format::Rgba8);
        assert!(renderer.upload_layer(&prepared(1, FaceSize::Board, 1, layer.clone())));
        // The card is 63 by 88 millimetres, its top edge 44 millimetres up. The battlefield
        // frame's name bar is a tenth of the card's width tall, its middle about 6.5 millimetres
        // below the top edge; its right part, past the name, is the blue palette's bar color.
        let picture = draw_card(&mut renderer, 1, 50.0);
        let bar = at(&picture, 18.0, 44.0 - 6.5);
        assert_near(
            bar,
            [0xb9, 0xd3, 0xea],
            24,
            &format!("{limits:?}: the name bar"),
        );
        // The rim beside it is the palette's deep blue.
        let rim = at(&picture, 0.0, 44.0 - 2.4);
        assert!(rim[2] > rim[0] + 60, "{limits:?}: the rim is {rim:?}");
    }
}

/// An uncompressed layer of `size` filled with `value`, every mipmap level included.
fn plain_layer(size: FaceSize, value: u8) -> Layer {
    let (width, height) = dimensions(size);
    let levels = (0..u32::BITS - width.max(height).leading_zeros())
        .map(|level| vec![value; ((width >> level).max(1) * (height >> level).max(1) * 4) as usize])
        .collect();
    Layer {
        format: Format::Rgba8,
        width,
        height,
        levels,
    }
}

#[test]
fn faces_that_arrive_during_play_wait_for_a_still_client() {
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        renderer.set_faces(&["", "a", "b", "c", "d", "e"], Format::Rgba8);
        for face in 1..=5 {
            renderer.hold(prepared(
                face,
                FaceSize::Board,
                face,
                plain_layer(FaceSize::Board, 255),
            ));
        }
        for _ in 0..3 {
            assert_eq!(renderer.upload_waiting(Wake::Now).count(), 0, "{limits:?}");
        }
        assert_eq!(renderer.waiting(), 5, "{limits:?}");
        for (frame, face) in (1..=5).enumerate() {
            let wake = if frame % 2 == 0 {
                Wake::Never
            } else {
                Wake::At(1)
            };
            let uploaded: Vec<AssetReady> = renderer.upload_waiting(wake).collect();
            assert_eq!(
                uploaded,
                [AssetReady::Face {
                    face: FaceId(face),
                    size: FaceSize::Board,
                    layer: face,
                }],
                "{limits:?}"
            );
        }
        assert_eq!(
            renderer.upload_waiting(Wake::Never).count(),
            0,
            "{limits:?}"
        );
        // The uploaded white face replaced its placeholder.
        assert_eq!(
            at(&draw_card(&mut renderer, 3, FAR), 0.0, 0.0),
            [255; 3],
            "{limits:?}"
        );
    }
}

#[test]
fn one_full_size_face_is_uploaded_a_frame_even_while_animating() {
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        renderer.set_faces(&["", "a", "b"], Format::Rgba8);
        renderer.hold(prepared(
            1,
            FaceSize::Board,
            1,
            plain_layer(FaceSize::Board, 9),
        ));
        for (face, layer) in [(1, 0), (2, 1)] {
            renderer.hold(prepared(
                face,
                FaceSize::Full,
                layer,
                plain_layer(FaceSize::Full, 9),
            ));
        }
        let sizes = |renderer: &mut Renderer, wake| -> Vec<FaceSize> {
            renderer
                .upload_waiting(wake)
                .map(|ready| match ready {
                    AssetReady::Face { size, .. } => size,
                    AssetReady::Glyphs { .. }
                    | AssetReady::Icons { .. }
                    | AssetReady::GalleryFace { .. } => {
                        panic!("an atlas page was uploaded")
                    }
                })
                .collect()
        };
        assert_eq!(
            sizes(&mut renderer, Wake::Now),
            [FaceSize::Full],
            "{limits:?}"
        );
        assert_eq!(
            sizes(&mut renderer, Wake::Now),
            [FaceSize::Full],
            "{limits:?}"
        );
        assert_eq!(sizes(&mut renderer, Wake::Now), [], "{limits:?}");
        assert_eq!(
            sizes(&mut renderer, Wake::Never),
            [FaceSize::Board],
            "{limits:?}"
        );
    }
}

#[test]
fn low_memory_frees_the_full_size_array_and_keeps_the_board_size_one() {
    for limits in BOTH {
        let mut renderer = Renderer::headless(limits).expect("a GPU adapter and device");
        renderer.set_faces(&["", "a", "b"], Format::Rgba8);
        for face in 1..=2 {
            let board = prepared(face, FaceSize::Board, face, plain_layer(FaceSize::Board, 9));
            assert!(renderer.upload_layer(&board), "{limits:?}");
            let full = prepared(
                face,
                FaceSize::Full,
                face - 1,
                plain_layer(FaceSize::Full, 9),
            );
            assert!(renderer.upload_layer(&full), "{limits:?}");
        }
        renderer.hold(prepared(
            1,
            FaceSize::Full,
            2,
            plain_layer(FaceSize::Full, 9),
        ));
        assert_eq!(renderer.uploaded(FaceSize::Full), 2, "{limits:?}");

        renderer.low_memory();
        assert_eq!(renderer.uploaded(FaceSize::Full), 0, "{limits:?}");
        assert_eq!(renderer.uploaded(FaceSize::Board), 2, "{limits:?}");
        assert_eq!(
            renderer.waiting(),
            0,
            "a full-size face asked for earlier is dropped"
        );
        // The board-size faces still draw, from the array that was kept.
        assert_eq!(
            at(&draw_card(&mut renderer, 1, FAR), 0.0, 0.0),
            [9; 3],
            "{limits:?}"
        );

        let full = prepared(2, FaceSize::Full, 0, plain_layer(FaceSize::Full, 9));
        assert!(renderer.upload_layer(&full), "{limits:?}");
        assert_eq!(renderer.uploaded(FaceSize::Full), 1, "{limits:?}");
        assert_eq!(renderer.uploaded(FaceSize::Board), 2, "{limits:?}");
    }
}
