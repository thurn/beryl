//! Sound: every file the manifest lists decodes, the looping tracks are cut at loops.json's
//! exact lengths, and an audio thread with no output starts and drops what it is sent.

use app_screens::screens::Music;
use beryl_3d::audio::files::{self, Manifest};
use beryl_3d::audio::{Audio, Output};
use kit_frame::frame::{Sound, SoundId};

#[test]
fn every_sound_effect_decodes() {
    let manifest = Manifest::read().unwrap();
    let effects = files::decode_effects(&manifest).unwrap();
    assert_eq!(effects.len(), game_play::sound::NAMES.len());
    for (clips, cue) in effects.iter().zip(&manifest.sounds) {
        assert_eq!(clips.len(), cue.variants);
        assert!(clips.iter().all(|clip| clip.frames > 0));
    }
}

/// Every variant of every music decodes, and each loop is cut at exactly its length in
/// loops.json, which the file is at least as long as.
#[test]
fn every_track_decodes_and_loops_at_its_exact_length() {
    let manifest = Manifest::read().unwrap();
    let mut variants = Vec::new();
    for music in files::MUSIC {
        for n in 0..manifest.track(music).variants {
            variants.push((music, n));
        }
    }
    let decoded: Vec<_> = std::thread::scope(|scope| {
        let workers: Vec<_> = variants
            .chunks(variants.len().div_ceil(4))
            .map(|chunk| {
                scope.spawn(|| {
                    chunk
                        .iter()
                        .map(|(music, n)| {
                            (*music, *n, files::decode(&files::music_path(*music, *n)))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect()
    });
    for (music, n, clip) in decoded {
        let clip = clip.unwrap();
        let Some(seconds) = manifest.track(music).loops.get(n) else {
            assert!(matches!(music, Music::Win | Music::Loss));
            continue;
        };
        let rate = f64::from(rodio::Source::sample_rate(&clip.buffer).get());
        let exact = (seconds * rate).round() as usize;
        assert!(
            clip.frames >= exact,
            "{music:?} {n}: {} < {exact}",
            clip.frames
        );
        assert_eq!(clip.cut(*seconds).frames, exact, "{music:?} {n}");
    }
}

#[test]
fn the_game_loops_are_listed_for_every_variant() {
    let manifest = Manifest::read().unwrap();
    let game = manifest.track(Music::Game);
    assert!(game.looped());
    assert_eq!(game.loops.len(), game.variants);
    assert!(!manifest.track(Music::Win).looped());
}

#[test]
fn with_no_output_the_thread_drops_what_it_is_sent() {
    let audio = Audio::start(Output::Silent);
    let sound = Sound {
        id: SoundId(0),
        theirs: false,
    };
    audio.levels(50, 50, false);
    audio.play(sound);
    audio.play(Sound {
        theirs: true,
        ..sound
    });
    audio.music(Some(Music::Game));
    audio.music(None);
    assert_eq!(audio.stop(), 4);
}
