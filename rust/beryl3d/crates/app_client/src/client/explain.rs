//! Reads the decisions behind one resolved node. The live app never enables this probe.

use beryl_protocol::protocol::{ObjectId, RowName, Side, ZoneName};
use beryl_protocol::script::Subject;
use game_probe::explain::{self, Probe};
use game_table::table::{PileShown, Placement, RowDecision, Scroller};
use kit_frame::frame::{ABILITY_IDS, Place, Resolved, Role};
use kit_probe::agent::tree::place_word;
use kit_space::arrange::Overflow;
use serde_json::{Value, json};

use super::{Client, features::Features};

const CRATES: &str = "rust/beryl3d/crates";

impl Client {
    pub(crate) fn enable_explanation(&mut self) {
        self.probe = Some(Probe::default());
    }

    pub(crate) fn explain(&self, element: &Resolved) -> Value {
        let profile = self.show.table().profile();
        let profile_file = format!("data/beryl3d/profiles/{}.json", profile.data().name);
        if element.place == Place::Layer
            && let Some(node) = self.layer.node_style(element.id)
        {
            return self.explain_screen(element, &node, profile, &profile_file);
        }
        self.explain_table(
            element,
            &profile_file,
            profile.data().table.banded.is_some(),
        )
    }

    fn explain_screen(
        &self,
        element: &Resolved,
        node: &kit_ui::layout::NodeStyle<'_>,
        profile: game_table::profiles::Profile<'_>,
        profile_file: &str,
    ) -> Value {
        let name = &profile.data().name;
        let owner = node
            .owner
            .or_else(|| node.float.and_then(Features::float_owner))
            .unwrap_or("game_hud::screens::game::Game");
        let (feature, directory) = owner_path(owner);
        let (view, view_file) =
            if owner == std::any::type_name::<game_hud::prompts::bar::PromptBar>() {
                (
                    "game_hud::prompts::bar::view::bar".to_owned(),
                    format!("{directory}/view.rs"),
                )
            } else {
                (format!("{owner}::view"), directory.clone())
            };
        let parent = node.parent.map(explain::layout);
        let control = match node.element.name.as_str() {
            "dock" => Some("dock"),
            "more" => Some("stacked prompt controls"),
            "Opponent's Turn" => Some("Opponent's Turn"),
            _ => node.parent.and_then(|parent| match parent.name.as_str() {
                "dock" => Some("dock"),
                "more" => Some("stacked prompt controls"),
                "prompt" if node.element.role == Role::Text => Some("prompt pill"),
                _ => None,
            }),
        };
        let attempts = self
            .probe
            .as_ref()
            .into_iter()
            .flat_map(|probe| &probe.attempts)
            .filter(|attempt| Some(attempt.control) == control)
            .collect::<Vec<_>>();
        let slot = attempts
            .iter()
            .find(|attempt| attempt.selected)
            .map(|attempt| attempt.slot)
            .or_else(|| {
                (owner.contains("::prompts::bar::") && profile.slot("prompt.bar").is_some())
                    .then_some("prompt.bar")
            });
        let shown_attempts: Vec<_> = attempts
            .iter()
            .take(12)
            .map(|attempt| {
                json!({
                    "slot": attempt.slot,
                    "bounds": bounds(attempt.bounds),
                    "selected": attempt.selected,
                    "blocked_by": attempt.blocked.iter().take(8).map(|obstacle| {
                        json!({"id": obstacle.id.0, "group": obstacle.group,
                            "bounds": bounds(obstacle.bounds)})
                    }).collect::<Vec<_>>(),
                    "blockers": attempt.blocked.len(),
                })
            })
            .collect();
        let index = self
            .frame
            .resolved
            .iter()
            .position(|node| node.id == element.id);
        let anchor = self
            .frame
            .anchors
            .iter()
            .find(|anchor| Some(anchor.element as usize) == index)
            .map(|anchor| format!("{:?}", anchor.anchor))
            .or_else(|| node.element.anchor.map(|anchor| format!("{anchor:?}")));
        let floating = profile
            .data()
            .hud
            .floating_prompt
            .as_ref()
            .filter(|_| control.is_some())
            .map(|metrics| (metrics, profile_file));
        json!({
            "place": {
                "space": "screen", "profile": name, "profile_file": profile_file,
                "slot": slot, "layout": parent, "anchor": anchor,
                "attempts": shown_attempts, "attempt_count": attempts.len(),
            },
            "look": explain::styles(node.element, &view, &view_file, floating),
            "motion": node.transitions,
            "owner": {"feature": feature, "directory": directory},
        })
    }

    fn explain_table(&self, element: &Resolved, profile_file: &str, banded: bool) -> Value {
        let table_now = self.features.bug().table_time(self.now);
        let slot = match element.role {
            Role::Card => u16::try_from(element.id.0).ok().map(ObjectId),
            _ => None,
        };
        let subject = match element.role {
            Role::Ability => element
                .id
                .0
                .checked_sub(ABILITY_IDS)
                .map(|id| Subject::Ability { id }),
            _ => slot.map(|slot| Subject::Object { slot }),
        };
        let step = subject.and_then(|subject| self.show.moving_step(subject, table_now));
        let motion: Vec<_> = step
            .iter()
            .map(game_play::play::record::StepRecord::describe)
            .chain(slot.into_iter().flat_map(|slot| {
                self.show
                    .table()
                    .transitions_for_probe(usize::from(slot.0), table_now)
                    .into_iter()
                    .map(str::to_owned)
            }))
            .collect();
        let mut place = match element.place {
            Place::Card { zone, side, row } => {
                let placement = self.show.placement_for_probe(slot, zone);
                table_place(
                    element.place,
                    (zone, side, row),
                    placement.as_ref(),
                    (profile_file, banded),
                    self.frame
                        .resolved
                        .iter()
                        .filter(|other| other.place == element.place)
                        .count(),
                )
            }
            _ => json!({"space": "screen", "container": place_word(element.place)}),
        };
        if let Some(index) = self
            .frame
            .resolved
            .iter()
            .position(|node| node.id == element.id)
            && let Some(decoration) = self
                .frame
                .decorations
                .iter()
                .find(|decoration| decoration.element as usize == index)
        {
            place["anchor"] = format!("{:?}", decoration.anchor).into();
        }
        let (feature, directory) = match element.place {
            Place::Card { row: Some(_), .. } => ("battlefield rows", "game_table/src/table/place"),
            Place::Card {
                zone: ZoneName::Hand,
                ..
            } => ("hand", "game_table/src/table"),
            Place::Card {
                zone: ZoneName::Stack,
                ..
            } => ("stack", "game_table/src/table"),
            Place::Card { .. } | Place::Revealed => ("piles", "game_table/src/table"),
            Place::Layer if matches!(element.role, Role::Arrow | Role::Mark) => {
                ("table marks", "game_play/src/light")
            }
            Place::Layer => ("table HUD", "game_hud/src/hud"),
        };
        let mut styles = Vec::new();
        if let Some(pose) = element.pose {
            if pose.scale.to_bits() != 1.0f32.to_bits() {
                styles.push(json!({"property": "scale", "value": pose.scale,
                    "source": {"kind": "view", "key": feature, "file": format!("{CRATES}/{directory}")}}));
            }
            if pose.opacity.to_bits() != 1.0f32.to_bits() {
                styles.push(json!({"property": "opacity", "value": pose.opacity,
                    "source": {"kind": "view", "key": feature, "file": format!("{CRATES}/{directory}")}}));
            }
        }
        json!({
            "place": place, "look": styles, "motion": motion,
            "owner": {"feature": feature, "directory": format!("{CRATES}/{directory}")},
        })
    }
}

fn bounds(rect: kit_base::geometry::Rect) -> [f32; 4] {
    [rect.x, rect.y, rect.width, rect.height]
}

fn owner_path(owner: &str) -> (&str, String) {
    let (module, feature) = owner.rsplit_once("::").unwrap_or((owner, owner));
    let (package, module) = module.split_once("::").unwrap_or((module, ""));
    let directory = if module == "screens::game" {
        "screens".to_owned()
    } else {
        module.replace("::", "/")
    };
    (feature, format!("{CRATES}/{package}/src/{directory}"))
}

fn table_place(
    place: Place,
    (zone, side, row): (ZoneName, Side, Option<RowName>),
    placement: Option<&Placement>,
    (file, banded): (&str, bool),
    drawn: usize,
) -> Value {
    let region = if banded {
        "table.banded"
    } else {
        "table.anchored"
    };
    let (policy, key) = match (zone, row, side) {
        (_, Some(RowName::Creatures), _) => ("row", "table.creatures".to_owned()),
        (_, Some(RowName::Lands), _) => ("row", "table.lands".to_owned()),
        (_, Some(RowName::Others), _) => ("row", "table.others".to_owned()),
        (ZoneName::Hand, _, Side::You) => ("fan", format!("{region}.hand")),
        (ZoneName::Hand, _, Side::Opponent) => ("fan", format!("{region}.opponent_hand")),
        (ZoneName::Stack, _, _) => ("cascade", format!("{region}.stack")),
        _ => ("pile", format!("{region}.piles")),
    };
    let row_decision =
        placement.and_then(|placement| row.and_then(|row| placement.row_decision(side, row)));
    let key = row_decision.map_or(key.as_str(), |decision| decision.region);
    let items = placement.map_or(drawn, |placement| {
        if let Some(count) = row_decision.map(|row| row.items).or_else(|| {
            placement
                .container_decision(zone, side)
                .map(|decision| decision.items)
        }) {
            return count;
        }
        if matches!(
            zone,
            ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile
        ) {
            return placement.piles[PileShown::index(side, zone)].count;
        }
        placement
            .rests
            .iter()
            .flatten()
            .filter(|rest| rest.place == place)
            .count()
    });
    let mut stages = Vec::new();
    let mut limits = Vec::new();
    if let Some(placement) = placement {
        let window = match (row, zone, side) {
            (Some(row), _, _) => Some(placement.rows.get(Scroller::Row(side, row))),
            (None, ZoneName::Hand, Side::You) => Some(placement.rows.hand),
            (None, ZoneName::Stack, _) => Some(placement.rows.stack),
            _ => None,
        };
        if let Some(decision) = row_decision {
            row_overflow(decision, &mut stages, &mut limits);
        } else if let Some(decision) = placement.container_decision(zone, side) {
            let (stage, reason) = overflow_text(decision.overflow);
            stages.push(stage);
            if let Some(reason) = reason {
                limits.push(format!(
                    "{reason}, with a {:.0}-point press target",
                    decision.press_points
                ));
            }
        } else if policy == "pile" && items > 1 {
            stages.push("piled");
            limits.push("ordered pile with its top card exposed".to_owned());
        }
        if stages.last() != Some(&"scrolled")
            && window.is_some_and(|window| window.before() + window.after() > 0)
        {
            stages.push("scrolled");
            limits.push(
                "the container cannot show every item at its minimum exposed strip".to_owned(),
            );
        }
    }
    if stages.is_empty() {
        stages.push("full size");
    }
    json!({
        "space": "table", "container": place_word(place), "policy": policy,
        "items": items, "stage": stages.last(), "stages": stages, "limits": limits,
        "region": {"key": key, "file": file},
    })
}

fn row_overflow(decision: RowDecision, stages: &mut Vec<&str>, limits: &mut Vec<String>) {
    if decision.fit.pile_groups > 0 {
        stages.push("piled");
        limits.push(format!(
            "identical cards share {} of {} groups",
            decision.fit.pile_groups, decision.groups
        ));
    }
    if decision.zoom < 1.0 {
        stages.push("scaled");
        limits.push(format!(
            "row zoom {:.3}, minimum {:?}",
            decision.zoom, decision.least_zoom
        ));
    }
    if decision.fit.overflow {
        stages.push("overlapped");
        let fit = decision.fit;
        limits.push(if fit.full_width > fit.available_width {
            format!(
                "{:.1} mm full width exceeds {:.1} mm of row space",
                fit.full_width, fit.available_width
            )
        } else {
            format!(
                "full placement exceeds the row boundary or blocks a {:.0}-point press target",
                fit.press_points
            )
        });
    }
}

fn overflow_text(overflow: Overflow) -> (&'static str, Option<&'static str>) {
    match overflow {
        Overflow::FullSize => ("full size", None),
        Overflow::Piled => ("piled", Some("identical items share a position")),
        Overflow::Overlapped => ("overlapped", Some("preferred spacing exceeds the region")),
        Overflow::Scaled => ("scaled", Some("full-sized items exceed the region")),
        Overflow::Scrolled => (
            "scrolled",
            Some("only a window of items fits in the region"),
        ),
        Overflow::Clipped => (
            "clipped",
            Some("only part of the container fits in the region"),
        ),
    }
}
