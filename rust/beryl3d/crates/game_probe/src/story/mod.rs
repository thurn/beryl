//! Table and feature stories share a header with the toolkit's stories. Authored tables build
//! protocol views; captured moments keep the messages needed to reconstruct the client state.

mod capture;
mod source;
mod table;

use std::fmt::Write;
use std::sync::Arc;

use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::Side;
use kit_probe::story::{Header, parse_header};

pub use source::Source;
#[cfg(feature = "stories")]
pub use table::TableStory;
pub use table::placeholder_records;

/// Inputs for a settings sheet story, with omitted controls taking the app defaults.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsStory {
    pub scale: Option<f32>,
    pub screen: Option<String>,
    pub confirm_concede: bool,
    pub game_over: bool,
    pub music: Option<u8>,
    pub effects: Option<u8>,
    pub muted: Option<bool>,
    pub full_control: Option<bool>,
    pub graphics: Option<String>,
}

/// A moment played from its own stream of protocol messages, without the recordings pack.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    /// Used to reproduce built-in driver cues on stress boards.
    pub source: String,
    pub at: u64,
    pub seat: Side,
    pub scale: f32,
    pub catch_up: bool,
    pub messages: Vec<FixtureMessage>,
    /// The story sources supplied by the host. Older captures omit this input.
    pub story_catalog: Option<Arc<[Source]>>,
    /// Driver actions taken at the captured moment before drawing the story.
    pub actions: Vec<String>,
}

/// The game and app story forms.
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    #[cfg(feature = "stories")]
    Table(TableStory),
    Settings(SettingsStory),
    /// A feature exercised through the full client over a sequence of recorded inputs.
    Replay(Vec<FixtureMessage>),
    Capture {
        feature: Option<String>,
        capture: Capture,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Story {
    pub header: Header,
    pub profile: Option<String>,
    pub body: Body,
}

impl Story {
    /// Reads a table or feature story, reporting errors with their source line.
    ///
    /// # Errors
    ///
    /// When the header or any input line is invalid.
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut lines = source.lines();
        let (kind, header) = parse_header(lines.next().unwrap_or_default())
            .ok_or("line 1: write story <table|feature> <width>x<height> <desktop|phone>")?;
        let lines: Vec<_> = lines
            .enumerate()
            .map(|(index, line)| (index + 2, line.trim()))
            .filter(|(_, line)| !line.is_empty() && !line.starts_with("//"))
            .collect();
        let profile = lines
            .first()
            .and_then(|(_, line)| line.strip_prefix("profile "))
            .map(str::to_owned);
        let lines = &lines[usize::from(profile.is_some())..];
        let (feature, inputs) = match kind {
            "table" => (None, lines),
            "feature" => {
                let Some(((index, line), inputs)) = lines.split_first() else {
                    return Err("line 2: write feature <id>".to_owned());
                };
                let id = line
                    .strip_prefix("feature ")
                    .filter(|id| !id.is_empty() && !id.contains(char::is_whitespace))
                    .ok_or_else(|| format!("line {index}: write feature <id>"))?;
                (Some(id), inputs)
            }
            _ => return Err(format!("line 1: unknown story kind {kind:?}")),
        };
        let body = if inputs
            .first()
            .is_some_and(|(_, line)| line.starts_with("capture "))
        {
            Body::Capture {
                feature: feature.map(str::to_owned),
                capture: Capture::parse(inputs)?,
            }
        } else {
            match feature {
                #[cfg(feature = "stories")]
                None => Body::Table(TableStory::parse(inputs, None)?),
                #[cfg(not(feature = "stories"))]
                None => return Err("authored tables must be compiled to a recording".to_owned()),
                Some(_) if inputs.first().is_some_and(|(_, line)| *line == "replay") => {
                    Body::Replay(capture::read_replay(&inputs[1..])?)
                }
                Some("settings") => Body::Settings(SettingsStory::parse(inputs)?),
                #[cfg(feature = "stories")]
                Some(id) if inputs.first().is_some_and(|(_, line)| *line == "manual") => {
                    Body::Table(TableStory::parse(inputs, Some(id))?)
                }
                Some(id) => return Err(format!("no authored inputs for feature {id:?}")),
            }
        };
        Ok(Self {
            header,
            profile,
            body,
        })
    }
}

fn header_text(header: Header, kind: &str, profile: Option<&str>) -> String {
    let mut text = header.line(kind);
    if let Some(profile) = profile {
        let _ = writeln!(text, "profile {profile}");
    }
    text
}

impl Capture {
    fn parse(lines: &[(usize, &str)]) -> Result<Self, String> {
        let (index, line) = lines[0];
        let mut words = line.split_whitespace();
        let _ = words.next();
        let values = (|| {
            let source = words.next()?.to_owned();
            let at = words.next()?.parse().ok()?;
            let seat = match words.next()? {
                "you" => Side::You,
                "opponent" => Side::Opponent,
                _ => return None,
            };
            let scale: f32 = words.next()?.parse().ok()?;
            let catch_up = match words.next()? {
                "catch-up" => true,
                "real-time" => false,
                _ => return None,
            };
            (scale.is_finite() && scale > 0.0 && words.next().is_none())
                .then_some((source, at, seat, scale, catch_up))
        })();
        let Some((source, at, seat, scale, catch_up)) = values else {
            return Err(format!(
                "line {index}: write capture <source> <nanoseconds> <seat> <scale> <catch-up|real-time>"
            ));
        };
        let mut inputs = &lines[1..];
        let story_catalog = if let Some((&(index, line), rest)) = inputs.split_first()
            && let Some(json) = line.strip_prefix("story-catalog ")
        {
            inputs = rest;
            let sources: Vec<Source> =
                serde_json::from_str(json).map_err(|error| format!("line {index}: {error}"))?;
            Some(sources.into())
        } else {
            None
        };
        let (actions, messages) = inputs.split_at(
            inputs
                .iter()
                .take_while(|(_, line)| line.starts_with("action "))
                .count(),
        );
        let actions = actions
            .iter()
            .map(|(_, line)| line["action ".len()..].to_owned())
            .collect();
        let messages = capture::read(messages)?;
        Ok(Self {
            source,
            at,
            seat,
            scale,
            catch_up,
            messages,
            story_catalog,
            actions,
        })
    }

    /// Writes a complete table or feature story. Keep protocol messages because the exact
    /// frame can depend on earlier transitions, prompt replies and effects still on screen.
    ///
    /// # Errors
    ///
    /// When the catalog or a recorded message cannot be written as JSON.
    pub fn write(
        &self,
        header: Header,
        feature: Option<&str>,
        profile: Option<&str>,
    ) -> Result<String, serde_json::Error> {
        let kind = if feature.is_some() {
            "feature"
        } else {
            "table"
        };
        let mut text = header_text(header, kind, profile);
        if let Some(id) = feature {
            let _ = writeln!(text, "feature {id}");
        }
        let seat = match self.seat {
            Side::You => "you",
            Side::Opponent => "opponent",
        };
        let pace = if self.catch_up {
            "catch-up"
        } else {
            "real-time"
        };
        let _ = writeln!(
            text,
            "capture {} {} {seat} {} {pace}",
            self.source, self.at, self.scale
        );
        if let Some(sources) = &self.story_catalog {
            let _ = writeln!(
                text,
                "story-catalog {}",
                serde_json::to_string(sources.as_ref())?
            );
        }
        for action in &self.actions {
            let _ = writeln!(text, "action {action}");
        }
        for message in &self.messages {
            capture::write(&mut text, message)?;
        }
        Ok(text)
    }
}

impl SettingsStory {
    /// Writes editable state and view inputs for a settings sheet.
    #[must_use]
    pub fn write(&self, header: Header, profile: Option<&str>) -> String {
        let mut text = header_text(header, "feature", profile);
        let _ = writeln!(text, "feature settings");
        if let Some(scale) = self.scale {
            let _ = writeln!(text, "scale {scale}");
        }
        if let Some(screen) = &self.screen {
            let _ = writeln!(text, "screen {screen}");
        }
        let _ = writeln!(
            text,
            "confirm-concede {}\ngame-over {}",
            self.confirm_concede, self.game_over
        );
        for (key, level) in [("music", self.music), ("effects", self.effects)] {
            if let Some(level) = level {
                let _ = writeln!(text, "{key} {level}");
            }
        }
        for (key, value) in [("muted", self.muted), ("full-control", self.full_control)] {
            if let Some(value) = value {
                let _ = writeln!(text, "{key} {value}");
            }
        }
        if let Some(graphics) = &self.graphics {
            let _ = writeln!(text, "graphics {graphics}");
        }
        text
    }

    fn parse(lines: &[(usize, &str)]) -> Result<Self, String> {
        let mut settings = Self::default();
        for &(index, line) in lines {
            settings
                .set(line)
                .map_err(|error| format!("line {index}: {error}"))?;
        }
        Ok(settings)
    }

    fn set(&mut self, line: &str) -> Result<(), String> {
        let (key, value) = line
            .split_once(' ')
            .ok_or("write a settings key followed by a value")?;
        let boolean = || {
            value
                .parse()
                .map_err(|_| format!("{key} takes true or false"))
        };
        let level = || {
            value
                .parse::<u8>()
                .ok()
                .filter(|&n| n <= 100)
                .ok_or_else(|| format!("{key} takes a level from 0 to 100"))
        };
        match key {
            "scale" => {
                self.scale = Some(
                    value
                        .parse::<f32>()
                        .ok()
                        .filter(|scale| scale.is_finite() && *scale > 0.0)
                        .ok_or("scale takes a positive number")?,
                );
            }
            "screen"
                if matches!(
                    value,
                    "start" | "game" | "lobby" | "table" | "gallery" | "testing"
                ) =>
            {
                self.screen = Some(value.to_owned());
            }
            "screen" => {
                return Err("screen takes start, game, lobby, table, gallery or testing".to_owned());
            }
            "confirm-concede" => self.confirm_concede = boolean()?,
            "game-over" => self.game_over = boolean()?,
            "music" => self.music = Some(level()?),
            "effects" => self.effects = Some(level()?),
            "muted" => self.muted = Some(boolean()?),
            "full-control" => self.full_control = Some(boolean()?),
            "graphics" if matches!(value, "automatic" | "baseline" | "enhanced") => {
                self.graphics = Some(value.to_owned());
            }
            "graphics" => return Err("graphics takes automatic, baseline or enhanced".to_owned()),
            _ => return Err(format!("unknown settings input {key:?}")),
        }
        Ok(())
    }
}
