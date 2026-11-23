//! `data/audio/manifest.json` names each cue and its variants. `data/audio/loops.json` gives
//! each looping track's exact length. Under `data/audio/assets`, sound effects are WAV files in
//! `sfx/` and music is AAC in M4A files in `music/`. Variant n of a cue is
//! `<cue>-<n>.<wav|m4a>`, as `just audio` writes it.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use app_screens::screens::Music;
use game_play::sound::NAMES;
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, Source};
use serde_json::Value;

/// The native app's list of cues.
const MANIFEST: &str = include_str!("../../../../../../data/audio/manifest.json");
/// The exact length of each variant of each looping track, in seconds.
const LOOPS: &str = include_str!("../../../../../../data/audio/loops.json");
/// A sound's most copies at once when its manifest entry gives none, as in `sound.ts`.
const VOICES: usize = 4;

/// Every music the client asks for, in the order `Manifest::music` holds them.
pub const MUSIC: [Music; 4] = [Music::Menu, Music::Game, Music::Win, Music::Loss];

/// The audio resources in a desktop release, or the checkout's `data/audio/assets`.
#[must_use]
pub fn folder() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Ok(executable) = std::env::current_exe()
        && let Some(macos) = executable.parent()
        && macos.ends_with("Contents/MacOS")
    {
        return macos.join("../Resources/audio");
    }
    #[cfg(target_os = "windows")]
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
        && parent.join("audio").is_dir()
    {
        return parent.join("audio");
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../data/audio/assets")
}

/// The manifest's key for `music`.
#[must_use]
pub fn music_name(music: Music) -> &'static str {
    match music {
        Music::Menu => "menu",
        Music::Game => "game",
        Music::Win => "win",
        Music::Loss => "loss",
    }
}

/// One sound effect's entry in the manifest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    /// Decibels added at playback: the files are all normalized to one peak or loudness.
    pub gain: f32,
    /// The most copies that play at once: a request while that many play is dropped.
    pub voices: usize,
    /// How many files the cue has, one of which is chosen at random each time.
    pub variants: usize,
}

/// One music's entry in the manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// The length of each variant's loop in seconds, from loops.json; empty for a sting, which
    /// plays once (the win and the loss).
    pub loops: Vec<f64>,
    pub variants: usize,
}

impl Track {
    /// Whether the music repeats.
    #[must_use]
    pub fn looped(&self) -> bool {
        !self.loops.is_empty()
    }
}

/// The manifest, read into the shapes the audio thread uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    /// The sound effects, the n-th being `SoundId(n)`: the manifest's `sounds` in file order.
    pub sounds: Vec<Cue>,
    /// The music, in the order of `MUSIC`.
    pub music: Vec<Track>,
}

impl Manifest {
    /// The manifest of the checkout the app was built from.
    ///
    /// # Errors
    ///
    /// When manifest.json or loops.json lacks an entry the client needs.
    pub fn read() -> Result<Self, String> {
        let manifest: Value = serde_json::from_str(MANIFEST)
            .map_err(|error| format!("data/audio/manifest.json: {error}"))?;
        let loops: Value = serde_json::from_str(LOOPS)
            .map_err(|error| format!("data/audio/loops.json: {error}"))?;
        let variants = |entry: &Value, name: &str| {
            entry["variants"]
                .as_array()
                .map(Vec::len)
                .filter(|count| *count > 0)
                .ok_or_else(|| format!("manifest.json: {name} has no variants"))
        };
        let sounds = NAMES
            .iter()
            .map(|name| {
                let entry = &manifest["sounds"][name];
                Ok(Cue {
                    gain: entry["gain"].as_f64().unwrap_or(0.0) as f32,
                    voices: entry["voices"]
                        .as_u64()
                        .map_or(VOICES, |voices| voices as usize),
                    variants: variants(entry, name)?,
                })
            })
            .collect::<Result<_, String>>()?;
        let music = MUSIC
            .iter()
            .map(|music| {
                let name = music_name(*music);
                let entry = &manifest["music"][name];
                let variants = variants(entry, name)?;
                let loops = if entry["loop"].as_bool().unwrap_or(false) {
                    let lengths: Vec<f64> = loops[name]
                        .as_array()
                        .map(|lengths| lengths.iter().filter_map(Value::as_f64).collect())
                        .unwrap_or_default();
                    if lengths.len() != variants {
                        return Err(format!(
                            "loops.json: {name} has {} lengths for {variants} variants",
                            lengths.len()
                        ));
                    }
                    lengths
                } else {
                    Vec::new()
                };
                Ok(Track { loops, variants })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { sounds, music })
    }

    /// The entry of `music`.
    #[must_use]
    pub fn track(&self, music: Music) -> &Track {
        let at = MUSIC.iter().position(|other| *other == music).unwrap_or(0);
        &self.music[at]
    }
}

/// The file of variant `n` of the sound effect `SoundId(id)`.
#[must_use]
pub fn sound_path(id: usize, n: usize) -> PathBuf {
    folder().join("sfx").join(format!("{}-{n}.wav", NAMES[id]))
}

/// The file of variant `n` of `music`.
#[must_use]
pub fn music_path(music: Music, n: usize) -> PathBuf {
    folder()
        .join("music")
        .join(format!("{}-{n}.m4a", music_name(music)))
}

/// A decoded sound: its samples in memory, shared by every copy that plays.
#[derive(Clone, Debug)]
pub struct Clip {
    pub buffer: SamplesBuffer,
    /// How many frames it has: samples over channels.
    pub frames: usize,
}

impl Clip {
    /// The first `seconds` of the clip, or all of it when it is shorter: a loop cut at its
    /// exact length, without the decoder's trailing padding, as `sound.ts` sets `loopEnd`.
    #[must_use]
    pub fn cut(&self, seconds: f64) -> Clip {
        let channels = self.buffer.channels();
        let rate = self.buffer.sample_rate();
        let frames = ((seconds * f64::from(rate.get())).round() as usize).min(self.frames);
        let samples: Vec<f32> = self
            .buffer
            .clone()
            .take(frames * usize::from(channels.get()))
            .collect();
        Clip {
            buffer: SamplesBuffer::new(channels, rate, samples),
            frames,
        }
    }

    /// How long the clip plays, in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.buffer.sample_rate().get())
    }
}

/// Decodes the file at `path` whole into memory.
///
/// # Errors
///
/// When the file cannot be read or decoded.
pub fn decode(path: &Path) -> Result<Clip, String> {
    let failed = |error: &dyn std::fmt::Display| format!("{}: {error}", path.display());
    let file = File::open(path).map_err(|error| failed(&error))?;
    let length = file.metadata().map_err(|error| failed(&error))?.len();
    let mut builder = Decoder::builder()
        .with_data(BufReader::new(file))
        .with_byte_len(length)
        .with_seekable(true)
        .with_gapless(true);
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        builder = builder.with_hint(extension);
    }
    let decoder = builder.build().map_err(|error| failed(&error))?;
    let channels = decoder.channels();
    let rate = decoder.sample_rate();
    let samples: Vec<f32> = decoder.collect();
    let frames = samples.len() / usize::from(channels.get());
    if frames == 0 {
        return Err(failed(&"no samples"));
    }
    Ok(Clip {
        buffer: SamplesBuffer::new(channels, rate, samples),
        frames,
    })
}

/// Decodes every variant of every sound effect, indexed by `SoundId` and then by variant.
///
/// # Errors
///
/// When a file cannot be read or decoded.
pub fn decode_effects(manifest: &Manifest) -> Result<Vec<Vec<Clip>>, String> {
    manifest
        .sounds
        .iter()
        .enumerate()
        .map(|(id, cue)| {
            (0..cue.variants)
                .map(|n| decode(&sound_path(id, n)))
                .collect()
        })
        .collect()
}

/// Decodes variant `n` of `music`, cut at its loop's exact length when it loops.
///
/// # Errors
///
/// When the file cannot be read or decoded.
pub fn decode_music(manifest: &Manifest, music: Music, n: usize) -> Result<Clip, String> {
    let clip = decode(&music_path(music, n))?;
    Ok(match manifest.track(music).loops.get(n) {
        Some(seconds) => clip.cut(*seconds),
        None => clip,
    })
}
