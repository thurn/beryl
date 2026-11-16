//! Sound (docs/beryl-3d.md, "Sound" and "Threads"): the effects the client lists in
//! `Frame::sounds` and the music it asks for with `Request::Music`, played as the web
//! interface's `ui/src/audio/sound.ts` plays them.
//!
//! The frame thread only sends messages. The audio thread owns the output device (`rodio`
//! over `cpal`, whose own callback thread mixes the samples) and every playing sound. At
//! startup a worker decodes every sound effect into memory, so that playing one never reads a
//! file; a music track is decoded on a worker when it is asked for, since the music decoded
//! whole would take about half a gigabyte.

pub mod files;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use beryl_scene::frame::Sound;
use beryl_scene::screens::Music;
use rodio::mixer::Mixer;
use rodio::source::Done;
use rodio::{Player, Source};

use files::{Clip, Manifest};

/// How far each copy of a sound strays from the file, so that repeats are not identical: up to
/// this fraction in pitch and this many decibels in level, as in `sound.ts`.
const PITCH_JITTER: f32 = 0.03;
const GAIN_JITTER_DB: f32 = 1.0;
/// How much quieter the opponent's sounds play, in decibels (`Sound::theirs`).
const THEIRS_DB: f32 = -4.0;
/// How long one music track takes to fade into the next.
const CROSSFADE: Duration = Duration::from_millis(1500);
/// How long a sting (the win or the loss) takes to fade in: it starts at once.
const STING_FADE: Duration = Duration::from_millis(20);
/// How often the audio thread wakes with no message, to move fades on and change tracks.
const POLL: Duration = Duration::from_millis(50);

/// Where the audio thread plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// The system's default output device.
    Device,
    /// Nowhere: the thread runs and decodes the effects, and drops every request to play. A run
    /// with no window uses this, so that agents' and checks' runs make no noise.
    Silent,
}

/// The audio thread, which plays what it is sent. Dropping it stops the thread.
pub struct Audio {
    sender: Sender<Message>,
    thread: Option<JoinHandle<usize>>,
}

enum Message {
    Play(Sound),
    Music(Option<Music>),
    Levels(Levels),
    /// The worker has decoded the effects.
    Effects(Result<Vec<Vec<Clip>>, String>),
    /// A worker has decoded a track that `ticket` asked for.
    Track {
        ticket: u64,
        use_for: Use,
        clip: Result<Clip, String>,
    },
    Stop,
}

/// What a decoded track is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    /// To play now.
    Start,
    /// To follow the track playing, in a rotation.
    Next,
}

/// The music's and the effects' levels as linear factors, 0 when muted.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Levels {
    music: f32,
    effects: f32,
}

impl Levels {
    fn of(music: u8, effects: u8, muted: bool) -> Self {
        let on = if muted { 0.0 } else { 1.0 };
        Self {
            music: f32::from(music.min(100)) / 100.0 * on,
            effects: f32::from(effects.min(100)) / 100.0 * on,
        }
    }
}

impl Audio {
    /// Starts the audio thread at the web interface's default levels.
    #[must_use]
    pub fn start(output: Output) -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread = {
            let sender = sender.clone();
            std::thread::Builder::new()
                .name("audio".to_owned())
                .spawn(move || run(output, &sender, &receiver))
        };
        let thread = match thread {
            Ok(thread) => Some(thread),
            Err(error) => {
                eprintln!(
                    "beryl3d: the audio thread could not start, so the app is silent: {error}"
                );
                None
            }
        };
        Self { sender, thread }
    }

    /// Plays a sound effect.
    pub fn play(&self, sound: Sound) {
        self.send(Message::Play(sound));
    }

    /// Plays `music` from now on, or none.
    pub fn music(&self, music: Option<Music>) {
        self.send(Message::Music(music));
    }

    /// Sets the levels, from 0 to 100, or silence when `muted`.
    pub fn levels(&self, music: u8, effects: u8, muted: bool) {
        self.send(Message::Levels(Levels::of(music, effects, muted)));
    }

    /// Stops the thread once it has handled every message sent before, and says how many
    /// requests to play it dropped for having no output.
    #[must_use]
    pub fn stop(mut self) -> usize {
        self.join()
    }

    fn send(&self, message: Message) {
        // The thread only ends when stopped, or panicked, which the join reports.
        let _ = self.sender.send(message);
    }

    fn join(&mut self) -> usize {
        self.send(Message::Stop);
        let Some(thread) = self.thread.take() else {
            return 0;
        };
        thread.join().unwrap_or_else(|_| {
            eprintln!("beryl3d: the audio thread panicked");
            0
        })
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.join();
    }
}

/// The output device, kept open while the thread runs.
struct Device {
    #[cfg(not(target_os = "android"))]
    _sink: rodio::MixerDeviceSink,
    mixer: Mixer,
}

/// Opens the default output device.
#[cfg(not(target_os = "android"))]
fn open() -> Result<Device, String> {
    let mut sink =
        rodio::DeviceSinkBuilder::open_default_sink().map_err(|error| error.to_string())?;
    // Closing the device at the app's end is no news.
    sink.log_on_drop(false);
    let mixer = sink.mixer().clone();
    Ok(Device { _sink: sink, mixer })
}

/// On Android the Kotlin host is to play the sound, since `cpal` links `AAudio`, which API
/// levels 24 and 25 lack; until it does, the app is silent there.
#[cfg(target_os = "android")]
fn open() -> Result<Device, String> {
    Err("Android's host does not play sound yet".to_owned())
}

/// The track playing.
struct Playing {
    player: Player,
    music: Music,
    clip: Clip,
    started: Instant,
    fade_in: Duration,
}

/// A track fading out, dropped once silent.
struct Fading {
    player: Player,
    from: f32,
    started: Instant,
}

/// The audio thread's state.
struct Thread {
    manifest: Manifest,
    device: Option<Device>,
    sender: Sender<Message>,
    effects: Option<Vec<Vec<Clip>>>,
    /// How many copies of each sound play now, which each copy lowers as it ends.
    copies: Vec<Arc<AtomicUsize>>,
    levels: Levels,
    /// The music asked for last.
    wanted: Option<Music>,
    /// Raised by every change of music, so that a track still decoding when another is asked
    /// for never plays.
    ticket: u64,
    /// The variant of each music that plays next, as `MUSIC` orders them.
    rotation: [Option<usize>; files::MUSIC.len()],
    playing: Option<Playing>,
    /// The next variant of the looping track playing, decoded ahead.
    next: Option<Clip>,
    fading: Vec<Fading>,
    random: u64,
    dropped: usize,
}

/// The audio thread's body: handles messages until told to stop, and returns how many requests
/// to play it dropped for having no output.
fn run(output: Output, sender: &Sender<Message>, receiver: &Receiver<Message>) -> usize {
    let manifest = match Manifest::read() {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("beryl3d: the app is silent: {error}");
            return drain(receiver);
        }
    };
    let device = match output {
        Output::Silent => None,
        Output::Device => match open() {
            Ok(device) => Some(device),
            Err(error) => {
                eprintln!("beryl3d: no audio device, so the app is silent: {error}");
                None
            }
        },
    };
    {
        let manifest = manifest.clone();
        let sender = sender.clone();
        let decoding = std::thread::Builder::new()
            .name("sound decoder".to_owned())
            .spawn(move || {
                let _ = sender.send(Message::Effects(files::decode_effects(&manifest)));
            });
        if let Err(error) = decoding {
            eprintln!("beryl3d: the sound effects could not be decoded: {error}");
        }
    }
    let mut thread = Thread {
        copies: (0..manifest.sounds.len())
            .map(|_| Arc::new(AtomicUsize::new(0)))
            .collect(),
        manifest,
        device,
        sender: sender.clone(),
        effects: None,
        levels: Levels::of(35, 70, false),
        wanted: None,
        ticket: 0,
        rotation: [None; files::MUSIC.len()],
        playing: None,
        next: None,
        fading: Vec::new(),
        random: seed(),
        dropped: 0,
    };
    loop {
        let message = match receiver.recv_timeout(POLL) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => {
                thread.poll();
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match message {
            Message::Play(sound) => thread.play(sound),
            Message::Music(music) => thread.music(music),
            Message::Levels(levels) => {
                thread.levels = levels;
                thread.poll();
            }
            Message::Effects(Ok(effects)) => thread.effects = Some(effects),
            Message::Effects(Err(error)) => {
                eprintln!("beryl3d: the sound effects could not be decoded: {error}");
            }
            Message::Track {
                ticket,
                use_for,
                clip,
            } => match clip {
                Err(error) => eprintln!("beryl3d: a track could not be decoded: {error}"),
                Ok(_) if ticket != thread.ticket => {}
                Ok(clip) => match use_for {
                    Use::Start => thread.start(clip),
                    Use::Next => thread.next = Some(clip),
                },
            },
            Message::Stop => break,
        }
    }
    thread.dropped
}

/// Counts the requests to play until told to stop, for a thread that has nothing to play
/// with.
fn drain(receiver: &Receiver<Message>) -> usize {
    let mut dropped = 0;
    while let Ok(message) = receiver.recv() {
        match message {
            Message::Play(_) | Message::Music(_) => dropped += 1,
            Message::Stop => break,
            _ => {}
        }
    }
    dropped
}

/// A seed for the variant and jitter choices, from the clock.
fn seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    nanos | 1
}

fn from_db(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Thread {
    /// A random number below `below`, from a xorshift generator.
    fn below(&mut self, below: usize) -> usize {
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        (self.random % below.max(1) as u64) as usize
    }

    /// A random number between -1 and 1.
    fn jitter(&mut self) -> f32 {
        self.below(2001) as f32 / 1000.0 - 1.0
    }

    /// Plays a sound effect as `play` in `sound.ts` does: a variant at random, slightly varied
    /// in pitch and level, unless the sound already plays at its most copies or effects are
    /// off.
    fn play(&mut self, sound: Sound) {
        let Some(device) = &self.device else {
            self.dropped += 1;
            return;
        };
        let mixer = device.mixer.clone();
        let id = usize::from(sound.id.0);
        let (true, Some(cue), Some(copies)) = (
            self.effects.is_some(),
            self.manifest.sounds.get(id).copied(),
            self.copies.get(id).map(Arc::clone),
        ) else {
            return;
        };
        if self.levels.effects <= 0.0 || copies.load(Ordering::Relaxed) >= cue.voices {
            return;
        }
        let variant = self.below(cue.variants);
        let Some(buffer) = (self.effects.as_ref())
            .and_then(|effects| effects.get(id)?.get(variant))
            .map(|clip| clip.buffer.clone())
        else {
            return;
        };
        let speed = 1.0 + self.jitter() * PITCH_JITTER;
        let theirs = if sound.theirs { THEIRS_DB } else { 0.0 };
        let gain = from_db(cue.gain + theirs + self.jitter() * GAIN_JITTER_DB);
        copies.fetch_add(1, Ordering::Relaxed);
        mixer.add(Done::new(
            buffer.speed(speed).amplify(self.levels.effects * gain),
            copies,
        ));
    }

    /// Asks for `music`, or silence: a change crossfades, and the same music plays on.
    fn music(&mut self, music: Option<Music>) {
        if music == self.wanted {
            return;
        }
        self.wanted = music;
        self.ticket += 1;
        self.next = None;
        if self.device.is_none() {
            self.dropped += 1;
            return;
        }
        match music {
            None => self.fade_out(),
            Some(music) => {
                let n = self.rotate(music);
                self.decode(music, n, Use::Start);
            }
        }
    }

    /// Which variant of `music` plays next: each in turn, from one chosen at random.
    fn rotate(&mut self, music: Music) -> usize {
        let at = files::MUSIC
            .iter()
            .position(|other| *other == music)
            .unwrap_or(0);
        let count = self.manifest.track(music).variants;
        let n = match self.rotation[at] {
            Some(n) => n,
            None => self.below(count),
        };
        self.rotation[at] = Some((n + 1) % count);
        n
    }

    /// Decodes variant `n` of `music` on a worker, which sends it back for `use_for`.
    fn decode(&self, music: Music, n: usize, use_for: Use) {
        let manifest = self.manifest.clone();
        let sender = self.sender.clone();
        let ticket = self.ticket;
        let decoding = std::thread::Builder::new()
            .name("music decoder".to_owned())
            .spawn(move || {
                let clip = files::decode_music(&manifest, music, n);
                let _ = sender.send(Message::Track {
                    ticket,
                    use_for,
                    clip,
                });
            });
        if let Err(error) = decoding {
            eprintln!("beryl3d: a track could not be decoded: {error}");
        }
    }

    /// Moves the track playing to the ones fading out.
    fn fade_out(&mut self) {
        if let Some(playing) = self.playing.take() {
            let from = playing.player.volume();
            self.fading.push(Fading {
                player: playing.player,
                from,
                started: Instant::now(),
            });
        }
    }

    /// Crossfades to `clip`, a variant of the music wanted. A loop plays twice before the next
    /// variant in its rotation takes over (`PLAYS_PER_TRACK` in `sound.ts`), or for ever when
    /// it has only one.
    fn start(&mut self, clip: Clip) {
        let (Some(device), Some(music)) = (&self.device, self.wanted) else {
            return;
        };
        let player = Player::connect_new(&device.mixer);
        player.set_volume(0.0);
        let track = self.manifest.track(music);
        let looped = track.looped();
        let rotates = looped && track.variants > 1;
        player.append(clip.buffer.clone());
        if looped {
            player.append(clip.buffer.clone());
        }
        self.fade_out();
        self.playing = Some(Playing {
            player,
            music,
            clip,
            started: Instant::now(),
            fade_in: if looped { CROSSFADE } else { STING_FADE },
        });
        if rotates {
            let n = self.rotate(music);
            self.decode(music, n, Use::Next);
        }
        self.poll();
    }

    /// Moves the fades on, ends silent tracks, and moves a looping track on: to the next
    /// variant of its rotation as its second play nears its end, or round again.
    fn poll(&mut self) {
        let now = Instant::now();
        self.fading.retain(|fading| {
            let gone = now.duration_since(fading.started).as_secs_f32() / CROSSFADE.as_secs_f32();
            fading
                .player
                .set_volume(fading.from * (1.0 - gone).max(0.0));
            gone < 1.0
        });
        let Some(playing) = &self.playing else {
            return;
        };
        let faded = (now.duration_since(playing.started).as_secs_f32()
            / playing.fade_in.as_secs_f32())
        .min(1.0);
        playing.player.set_volume(self.levels.music * faded);
        let track = self.manifest.track(playing.music);
        if !track.looped() || playing.player.len() > 1 {
            return;
        }
        if track.variants == 1 {
            playing.player.append(playing.clip.buffer.clone());
            return;
        }
        let left = playing.clip.seconds() - playing.player.get_pos().as_secs_f64();
        if left <= CROSSFADE.as_secs_f64()
            && let Some(next) = self.next.take()
        {
            self.start(next);
        }
    }
}
