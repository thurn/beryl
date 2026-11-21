//! The parts of a recording that affect interface coverage.

mod table;
mod words;

use std::collections::BTreeMap;

use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::{CLICK, FixtureMessage, GAME_UPDATE};
use beryl_protocol::prompt::{ClickReply, PromptView, ShapeBody, shape_name};
use beryl_protocol::protocol::{
    ObjectView, PlayerState, PlayerView, Side, StackItem, Status, ZoneName,
};
use beryl_protocol::script::{Act, Notice, Script, Subject};
use game_hud::hud::log::Logged;
use game_hud::prompts::prompt_title;
use game_table::table::Window;
use serde_json::{Value, json};

use self::table::{CardGroup, zone_groups};
use self::words::Words;

/// The ordered layout, motion and text shapes of one seat's recording.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Signature(Vec<u8>);

/// Measures the shapes in a recording's messages for the given desktop and phone windows.
/// The measurement cache is reused between recordings, but each recording starts a new log.
pub struct Measure {
    words: Words,
}

impl Measure {
    #[must_use]
    pub fn new(windows: &[Window]) -> Self {
        Self {
            words: Words::new(windows),
        }
    }

    /// The signature of one seat, in message order.
    ///
    /// # Errors
    ///
    /// When an update or click reply cannot be decoded.
    pub fn recording(&mut self, messages: &[FixtureMessage]) -> Result<Signature, String> {
        let mut bytes = Vec::new();
        let mut log = Logged::default();
        let mut acting_for = false;
        for (index, message) in messages.iter().enumerate() {
            let shape = match message {
                FixtureMessage::Event { event, payload } if event == GAME_UPDATE => {
                    let update: GameUpdate = serde_json::from_value(payload.clone())
                        .map_err(|error| format!("view message {index}: {error}"))?;
                    acting_for = matches!(&update.view.status,
                        Status::Deciding { decision } if decision.acting_for.is_some());
                    self.view(&update.view, &mut log)
                }
                FixtureMessage::Response { command, value, .. } if command == CLICK => {
                    match serde_json::from_value::<ClickReply>(value.clone())
                        .map_err(|error| format!("click reply {index}: {error}"))?
                    {
                        ClickReply::Open { prompt } => {
                            json!(["click", self.prompt(&prompt, acting_for)])
                        }
                        ClickReply::Answered => json!(["answered"]),
                    }
                }
                _ => continue,
            };
            serde_json::to_writer(&mut bytes, &shape).expect("a shape serializes");
            bytes.push(b'\n');
        }
        self.words.end_recording();
        Ok(Signature(bytes))
    }

    fn view(&mut self, view: &PlayerView, log: &mut Logged) -> Value {
        log.take(view);
        let zones: Vec<_> = zone_groups(view)
            .iter()
            .map(|groups| {
                groups
                    .iter()
                    .map(|group| card_group(group, &mut self.words))
                    .collect::<Vec<_>>()
            })
            .collect();
        let sideboards = [Side::You, Side::Opponent].map(|side| {
            view.objects
                .iter()
                .filter(|object| {
                    object.zone == ZoneName::Sideboard
                        && object.zone_side.unwrap_or(object.owner) == side
                })
                .map(|object| object_shape(object, &mut self.words))
                .collect::<Vec<_>>()
        });
        let stack: Vec<_> = view.stack.iter().map(stack_item).collect();
        let inspector: Vec<_> = view
            .objects
            .iter()
            .filter_map(|object| {
                let name = object.name.as_deref()?;
                Some([
                    Some(self.words.inspector(name)),
                    object
                        .note
                        .as_deref()
                        .map(|note| self.words.inspector(note)),
                    object
                        .chosen
                        .as_ref()
                        .map(|chosen| self.words.inspector(&format!("Chosen: {chosen}"))),
                    object
                        .really
                        .as_ref()
                        .map(|name| self.words.inspector(&format!("Copy. Card: {name}"))),
                ])
            })
            .collect();
        let log_lines: Vec<_> = log
            .entries()
            .iter()
            .map(|entry| self.words.log(&entry.line))
            .collect();
        let manual_log: Vec<_> = view
            .table
            .iter()
            .flat_map(|table| &table.log)
            .map(|line| {
                json!([
                    line.struck,
                    line.undoable,
                    self.words.manual_log(&line.by),
                    self.words.manual_log(&line.text)
                ])
            })
            .collect();
        json!([
            self.status(&view.status),
            view.turn.number == 0,
            view.turn.step,
            view.turn.active,
            view.handling,
            view.pending_attackers.as_ref().map(Vec::len),
            view.undone,
            zones,
            sideboards,
            stack,
            [
                player_shape(&view.players.you),
                player_shape(&view.players.opponent)
            ],
            inspector,
            log_lines,
            manual_log,
            view.script
                .as_ref()
                .map(|script| script_shape(script, &mut self.words)),
        ])
    }

    fn status(&mut self, status: &Status) -> Value {
        match status {
            Status::Deciding { decision } => {
                let mut candidates = BTreeMap::<String, usize>::new();
                for candidate in &decision.candidates {
                    let wire =
                        serde_json::to_value(&candidate.option).expect("candidate serializes");
                    let kind = wire["kind"].as_str().expect("candidate has a kind");
                    *candidates.entry(kind.to_owned()).or_default() += 1;
                }
                json!([
                    "deciding",
                    candidates,
                    self.prompt(&decision.prompt, decision.acting_for.is_some()),
                    decision.halt,
                    decision.cancellable,
                    decision.undo > 0,
                    decision.acting_for
                ])
            }
            Status::Thinking => json!("thinking"),
            Status::Free => json!("free"),
            Status::Playing => json!("playing"),
            Status::Stopped => json!("stopped"),
            Status::Over { .. } => json!("over"),
        }
    }

    fn prompt(&mut self, prompt: &PromptView, acting_for: bool) -> Value {
        let purpose =
            match prompt.purpose.name() {
                name @ ("act" | "mana" | "mulligan" | "play-or-draw" | "priority"
                | "put-on-bottom") => Some(name),
                _ => None,
            };
        let buttons: Vec<_> = prompt
            .buttons
            .iter()
            .map(|button| {
                let id = serde_json::to_value(button.id).expect("button id serializes");
                json!([
                    id["kind"],
                    button.enabled,
                    button.primary,
                    self.words.prompt(&button.label),
                    self.words.prompt(&button.caption)
                ])
            })
            .collect();
        let body = match &prompt.body {
            ShapeBody::Pick {
                tray,
                least,
                most,
                picked,
                warned,
            } => json!([tray.len(), least, most, picked.len(), warned.len()]),
            ShapeBody::Order { items } => json!(items.len()),
            ShapeBody::Split { piles } => json!(
                piles
                    .iter()
                    .map(|pile| json!([
                        pile.place,
                        pile.cards.len(),
                        pile.exactly,
                        pile.closed,
                        self.words.prompt(&pile.label)
                    ]))
                    .collect::<Vec<_>>()
            ),
            ShapeBody::YesNo { about, places } => json!([about.len(), places]),
            ShapeBody::Number { values, detail, .. } => json!([
                values.len(),
                detail.as_deref().map(|words| self.words.prompt(words))
            ]),
            ShapeBody::Mode { about } => json!(about.len()),
            ShapeBody::Catalog { entries, chosen } => json!([entries.len(), chosen.is_some()]),
            ShapeBody::Divide {
                total,
                left,
                shares,
            } => json!([total, left, shares.len()]),
            ShapeBody::Pay { cost, left } => json!([
                cost == left,
                self.words.prompt(cost),
                self.words.prompt(left)
            ]),
            ShapeBody::Declare {
                arrow,
                pairs,
                lined,
                aiming,
            } => json!([arrow, pairs.len(), lined, aiming.len()]),
            ShapeBody::Candidates => Value::Null,
        };
        let mut marks = BTreeMap::<String, usize>::new();
        for marked in &prompt.marks {
            let key = json!([subject_kind(marked.on), marked.mark]).to_string();
            *marks.entry(key).or_default() += 1;
        }
        let previews: Vec<_> = prompt
            .previews
            .iter()
            .map(|preview| preview.taps.len())
            .collect();
        json!([
            shape_name(prompt.shape()),
            purpose,
            prompt.part,
            prompt.count,
            prompt.about.len(),
            prompt.from,
            marks,
            previews,
            buttons,
            prompt.body.shape().map(shape_name),
            body,
            self.words.prompt(&prompt_title(prompt, acting_for)),
        ])
    }
}

fn card_group(group: &CardGroup<'_>, words: &mut Words) -> [Vec<(Value, usize)>; 3] {
    [&group.pile, &group.left, &group.right].map(|cards| {
        let mut runs: Vec<(Value, usize)> = Vec::new();
        for &card in cards {
            let shape = object_shape(card, words);
            if let Some((previous, count)) = runs.last_mut()
                && *previous == shape
            {
                *count += 1;
            } else {
                runs.push((shape, 1));
            }
        }
        runs
    })
}

fn object_shape(object: &ObjectView, words: &mut Words) -> Value {
    use beryl_protocol::protocol::Combat;
    let combat = match &object.combat {
        Some(Combat::Attacking {
            planeswalker,
            target_departed,
            blocked,
        }) => json!([
            "attacking",
            planeswalker.is_some(),
            target_departed,
            blocked
        ]),
        Some(Combat::Blocking { attacker }) => json!(["blocking", attacker.is_some()]),
        None => Value::Null,
    };
    let named_badges: Vec<_> = object
        .named_counters
        .iter()
        .filter(|counter| counter.count != 0)
        .map(|counter| {
            (
                words.counter_label_shape(&counter.name),
                counter.count.clamp(-99, 99),
            )
        })
        .collect();
    let flags = [
        object.name.is_some(),
        object.tapped,
        object.token,
        object.summoning_sick,
        object.damage != 0,
        object.phased_out,
        object.doesnt_untap,
        object.hidden_pick,
        object
            .really
            .as_deref()
            .is_some_and(|name| !name.is_empty()),
        object
            .chosen
            .as_deref()
            .is_some_and(|name| !name.is_empty()),
        object.note.as_deref().is_some_and(|note| !note.is_empty()),
        object.owner != object.controller,
        object.pt.is_some(),
        object.controller == Side::Opponent,
    ];
    let bits = flags
        .iter()
        .rev()
        .fold(0_u16, |bits, &flag| (bits << 1) | u16::from(flag));
    json!([
        bits,
        object.facing,
        object.face,
        combat,
        object
            .counters
            .iter()
            .map(|counter| (&counter.kind, counter.count.clamp(-99, 99)))
            .collect::<Vec<_>>(),
        named_badges,
        object.keywords.len()
    ])
}

fn player_shape(player: &PlayerState) -> Value {
    let mana = player.mana;
    let floating = [
        mana.white,
        mana.blue,
        mana.black,
        mana.red,
        mana.green,
        mana.colorless,
    ]
    .map(|count| count > 0);
    json!([
        player.mulligan,
        player.emblems.len(),
        player.named_counters.len(),
        floating,
    ])
}

fn stack_item(item: &StackItem) -> Value {
    match item {
        StackItem::Spell {
            targets, x, paid, ..
        } => json!(["spell", targets.len(), *x > 0, paid.len()]),
        StackItem::Ability {
            controller,
            triggered,
            source,
            targets,
            x,
            manual,
            ..
        } => json!([
            "ability",
            controller,
            triggered,
            source.is_some(),
            targets.len(),
            *x > 0,
            manual.is_some()
        ]),
    }
}

fn subject_kind(subject: Subject) -> &'static str {
    match subject {
        Subject::Object { .. } => "object",
        Subject::Ability { .. } => "ability",
        Subject::Player { player: Side::You } => "you",
        Subject::Player {
            player: Side::Opponent,
        } => "opponent",
    }
}

fn notice_shape(notice: &Notice) -> Value {
    match notice {
        Notice::Turn { active } => json!(["turn", active]),
        Notice::Scry {
            player,
            top,
            bottom,
        } => json!(["scry", player, top, bottom]),
        Notice::TopOrBottom { player, top } => json!(["topOrBottom", player, top]),
        Notice::Coin { player, won, .. } => json!(["coin", player, won]),
        Notice::Roll { sides, outcomes } => json!(["roll", sides, outcomes.len()]),
        Notice::Shuffle => json!("shuffle"),
        Notice::Rewind { .. } => json!("rewind"),
        Notice::Phase { phase } => json!(["phase", phase]),
        Notice::TableTurn { .. } => json!("tableTurn"),
        Notice::Game { .. } => json!("game"),
    }
}

fn script_shape(script: &Script, words: &mut Words) -> Value {
    let beats: Vec<_> = script
        .beats
        .iter()
        .map(|beat| {
            beat.iter()
                .map(|step| {
                    let motion = match &step.act {
                        Act::Fly {
                            from, why, object, ..
                        }
                        | Act::Appear {
                            from, why, object, ..
                        } => json!([from, why, object.zone]),
                        Act::Vanish {
                            from, why, subject, ..
                        } => json!([from, why, subject_kind(*subject)]),
                        Act::Strike { to, .. } => json!(subject_kind(*to)),
                        Act::Streak {
                            from, to, stream, ..
                        } => json!([from.map(subject_kind), subject_kind(*to), stream]),
                        Act::Point {
                            span,
                            arrow,
                            from,
                            to,
                        } => json!([span, arrow, subject_kind(*from), subject_kind(*to)]),
                        Act::Mark { span, mark, on } => json!([span, mark, subject_kind(*on)]),
                        Act::Lift { span, handling } => json!([span, handling]),
                        Act::Announce { notice } => notice_shape(notice),
                        _ => Value::Null,
                    };
                    json!([
                        step.act.verb(),
                        step.cause.map(subject_kind),
                        motion,
                        step.act.object().map(|object| object_shape(object, words))
                    ])
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let timing: Vec<_> = script
        .timing
        .iter()
        .map(|timing| {
            let effects: Vec<_> = timing
                .effects
                .iter()
                .map(|effect| {
                    json!([
                        effect.name,
                        effect.when,
                        effect.on.map(subject_kind),
                        effect.into,
                        effect.duration,
                        effect.delay,
                        effect.curve
                    ])
                })
                .collect();
            json!([
                timing.case,
                timing.duration,
                timing.delay,
                timing.stagger,
                timing.curve,
                timing.each,
                timing.with,
                timing.after.len(),
                timing.hold,
                timing.leads,
                timing.passes,
                timing.alone,
                timing.opens,
                timing.ahead,
                timing.closes,
                timing.room,
                timing.phases,
                effects
            ])
        })
        .collect();
    json!([
        script.reset.as_ref().map(|reset| reset.abilities.len()),
        beats,
        timing
    ])
}
