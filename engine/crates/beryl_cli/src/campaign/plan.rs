//! Campaign plan parsing, validation, and `--smoke` transformation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use beryl_search::DEEP_SEARCHER;
use serde::{Deserialize, Serialize};

use crate::blunders::BUDGETS;
use crate::game::parse_work;
use crate::ladder::{self, Bar, PROXY};
use crate::match_log::rung_name;

/// A work budget in a plan file: either a shorthand string (`"3M"`, `"10k"`) or an integer step
/// count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkSpec {
    Text(String),
    Steps(u64),
}

impl WorkSpec {
    pub fn parse(&self) -> Result<u64, String> {
        match self {
            WorkSpec::Text(s) => parse_work(s).ok_or_else(|| format!("invalid work budget {s:?}")),
            WorkSpec::Steps(n) if *n > 0 => Ok(*n),
            WorkSpec::Steps(n) => Err(format!("work budget must be positive, got {n}")),
        }
    }

    pub fn as_arg(&self) -> String {
        match self {
            WorkSpec::Text(s) => s.clone(),
            WorkSpec::Steps(n) => rung_name(*n),
        }
    }
}

/// A campaign plan file (`data/campaigns/<name>.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub name: String,
    pub stages: Vec<Stage>,
}

/// One stage of a campaign plan, in priority order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    #[serde(flatten)]
    pub kind: StageKind,
}

impl Stage {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            StageKind::Positions { .. } => "positions",
            StageKind::Gate { .. } => "gate",
            StageKind::Match { .. } => "match",
            StageKind::ValueTest { .. } => "value-test",
        }
    }

    pub fn is_value_test(&self) -> bool {
        matches!(self.kind, StageKind::ValueTest { .. })
    }

    pub fn is_match(&self) -> bool {
        matches!(self.kind, StageKind::Match { .. })
    }
}

fn default_value_seed() -> u64 {
    1
}

/// Stage kind and parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StageKind {
    Positions {
        games: u32,
        #[serde(default)]
        seed: u64,
        work: WorkSpec,
    },
    Gate {
        challenger: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        champion: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        claims: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seeds: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        work: Option<String>,
    },
    Match {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        challenger: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        champion: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        work: Option<WorkSpec>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bar: Option<String>,
        cap: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extends: Option<String>,
    },
    ValueTest {
        matches: Vec<String>,
        #[serde(default = "default_value_seed")]
        seed: u64,
    },
}

/// A validated match stage with inherited fields filled in and its root directory id resolved.
#[derive(Clone, Debug)]
pub struct ResolvedMatch {
    pub root_id: String,
    pub challenger: String,
    pub champion: String,
    pub work: String,
    pub bar: String,
    pub cap: u32,
    pub seed: u64,
    pub extends: Option<String>,
}

const RESERVED_IDS: [&str; 4] = ["campaign.json", "state.jsonl", "report.txt", "lock"];

fn valid_dir_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Replaces any `@budget` suffix on a search family with `@10k`; fixed agents stay as they are.
fn smoke_entrant(name: &str) -> String {
    match name.rsplit_once('@') {
        Some((family, _)) => format!("{family}@10k"),
        None => name.to_owned(),
    }
}

impl Plan {
    /// Reads, parses, and validates a plan file before anything runs.
    pub fn load(path: &Path) -> Result<Plan, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let plan: Plan =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        plan.validate()?;
        Ok(plan)
    }

    /// Validates every stage, resolving every family name with `ladder::resolve` and every
    /// `extends` and `matches` reference, and fails listing every error.
    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        if !valid_dir_name(&self.name) {
            errors.push(format!("invalid plan name {:?}", self.name));
        }
        if self.stages.is_empty() {
            errors.push("plan has no stages".to_owned());
        }

        let mut seen_ids = BTreeSet::new();
        let mut match_stages: BTreeMap<&str, (ResolvedMatch, u32)> = BTreeMap::new();

        for stage in &self.stages {
            let id = stage.id.as_str();
            if !valid_dir_name(id) || RESERVED_IDS.contains(&id) {
                errors.push(format!("invalid stage id {id:?}"));
            } else if !seen_ids.insert(id) {
                errors.push(format!("duplicate stage id {id:?}"));
            }

            match &stage.kind {
                StageKind::Positions { games, work, .. } => {
                    if *games == 0 {
                        errors.push(format!("stage {id}: games must be at least 1"));
                    }
                    if let Err(e) = work.parse() {
                        errors.push(format!("stage {id}: {e}"));
                    }
                }
                StageKind::Gate {
                    challenger,
                    champion,
                    seeds,
                    work,
                    ..
                } => {
                    validate_gate_family(id, "challenger", challenger, &mut errors);
                    if let Some(champ) = champion {
                        validate_gate_family(id, "champion", champ, &mut errors);
                    }
                    if *seeds == Some(0) {
                        errors.push(format!("stage {id}: seeds must be at least 1"));
                    }
                    if let Some(w) = work {
                        for part in w.split(',') {
                            if parse_work(part).is_none() {
                                errors.push(format!("stage {id}: invalid gate work {part:?}"));
                            }
                        }
                    }
                }
                StageKind::Match { .. } => {
                    validate_match_stage(id, &stage.kind, &mut match_stages, &mut errors);
                }
                StageKind::ValueTest { matches, .. } => {
                    if matches.is_empty() {
                        errors.push(format!("stage {id}: value-test lists no matches"));
                    }
                    for m in matches {
                        if !match_stages.contains_key(m.as_str()) {
                            errors.push(format!(
                                "stage {id}: matches references {m:?}, which is not an earlier match stage"
                            ));
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Resolves match stage `id`, inheriting fields from any `extends` chain.
    pub fn resolved_match(&self, id: &str) -> Option<ResolvedMatch> {
        let mut by_id: BTreeMap<&str, ResolvedMatch> = BTreeMap::new();
        for stage in &self.stages {
            if let StageKind::Match {
                challenger,
                champion,
                work,
                bar,
                cap,
                seed,
                extends,
            } = &stage.kind
            {
                let parent = extends.as_deref().and_then(|p| by_id.get(p)).cloned();
                let resolved = ResolvedMatch {
                    root_id: parent
                        .as_ref()
                        .map_or_else(|| stage.id.clone(), |p| p.root_id.clone()),
                    challenger: challenger
                        .clone()
                        .or_else(|| parent.as_ref().map(|p| p.challenger.clone()))
                        .unwrap_or_default(),
                    champion: champion
                        .clone()
                        .or_else(|| parent.as_ref().map(|p| p.champion.clone()))
                        .unwrap_or_else(|| DEEP_SEARCHER.to_string()),
                    work: work
                        .as_ref()
                        .map(WorkSpec::as_arg)
                        .or_else(|| parent.as_ref().map(|p| p.work.clone()))
                        .unwrap_or_else(|| rung_name(PROXY)),
                    bar: bar
                        .clone()
                        .or_else(|| parent.as_ref().map(|p| p.bar.clone()))
                        .unwrap_or_default(),
                    cap: *cap,
                    seed: seed
                        .or_else(|| parent.as_ref().map(|p| p.seed))
                        .unwrap_or(0),
                    extends: extends.clone(),
                };
                if stage.id == id {
                    return Some(resolved);
                }
                by_id.insert(stage.id.as_str(), resolved);
            }
        }
        None
    }

    /// The `--smoke` copy of this plan: every budget replaced by 10k, every cap by 4,
    /// `positions` by 4 games, and `gate` by `--seeds 1 --work 3k,10k`.
    #[must_use]
    pub fn smoke(&self) -> Plan {
        let stages = self
            .stages
            .iter()
            .map(|stage| {
                let kind = match &stage.kind {
                    StageKind::Positions { seed, .. } => StageKind::Positions {
                        games: 4,
                        seed: *seed,
                        work: WorkSpec::Text("10k".to_owned()),
                    },
                    StageKind::Gate {
                        challenger,
                        champion,
                        claims,
                        ..
                    } => StageKind::Gate {
                        challenger: challenger.clone(),
                        champion: champion.clone(),
                        claims: claims.clone(),
                        seeds: Some(1),
                        work: Some("3k,10k".to_owned()),
                    },
                    StageKind::Match {
                        challenger,
                        champion,
                        bar,
                        seed,
                        extends,
                        ..
                    } => StageKind::Match {
                        challenger: challenger.as_deref().map(smoke_entrant),
                        champion: champion.as_deref().map(smoke_entrant),
                        work: Some(WorkSpec::Text("10k".to_owned())),
                        bar: bar.clone(),
                        cap: 4,
                        seed: *seed,
                        extends: extends.clone(),
                    },
                    StageKind::ValueTest { matches, seed } => StageKind::ValueTest {
                        matches: matches.clone(),
                        seed: *seed,
                    },
                };
                Stage {
                    id: stage.id.clone(),
                    kind,
                }
            })
            .collect();
        Plan {
            name: self.name.clone(),
            stages,
        }
    }

    /// The id of the most recent `positions` stage occurring before `stage_id`, if any.
    pub fn positions_before(&self, stage_id: &str) -> Option<&str> {
        let mut found = None;
        for stage in &self.stages {
            if stage.id == stage_id {
                return found;
            }
            if matches!(stage.kind, StageKind::Positions { .. }) {
                found = Some(stage.id.as_str());
            }
        }
        None
    }
}

fn validate_gate_family(stage_id: &str, role: &str, name: &str, errors: &mut Vec<String>) {
    match ladder::resolve(name, BUDGETS[0]) {
        Ok(resolved) if resolved.contains('@') => {}
        Ok(_) => errors.push(format!(
            "stage {stage_id}: gate {role} {name:?} must be a search family"
        )),
        Err(e) => errors.push(format!("stage {stage_id}: gate {role} {name:?}: {e}")),
    }
}

fn validate_match_stage<'a>(
    id: &'a str,
    kind: &StageKind,
    match_stages: &mut BTreeMap<&'a str, (ResolvedMatch, u32)>,
    errors: &mut Vec<String>,
) {
    let StageKind::Match {
        challenger,
        champion,
        work,
        bar,
        cap,
        seed,
        extends,
    } = kind
    else {
        return;
    };

    let parent = extends.as_deref().and_then(|parent_id| {
        if let Some(p) = match_stages.get(parent_id) {
            Some(p.clone())
        } else {
            errors.push(format!(
                "stage {id}: extends {parent_id:?}, which is not an earlier match stage"
            ));
            None
        }
    });

    let eff_work_str = work
        .as_ref()
        .map(WorkSpec::as_arg)
        .or_else(|| parent.as_ref().map(|(p, _)| p.work.clone()))
        .unwrap_or_else(|| rung_name(PROXY));
    let eff_work = match work.as_ref() {
        Some(w) => match w.parse() {
            Ok(v) => v,
            Err(e) => {
                errors.push(format!("stage {id}: {e}"));
                PROXY
            }
        },
        None => parse_work(&eff_work_str).unwrap_or(PROXY),
    };

    let eff_challenger = challenger
        .clone()
        .or_else(|| parent.as_ref().map(|(p, _)| p.challenger.clone()));
    let eff_champion = champion
        .clone()
        .or_else(|| parent.as_ref().map(|(p, _)| p.champion.clone()))
        .unwrap_or_else(|| DEEP_SEARCHER.to_string());
    let eff_bar = bar
        .clone()
        .or_else(|| parent.as_ref().map(|(p, _)| p.bar.clone()));
    let eff_seed = seed
        .or_else(|| parent.as_ref().map(|(p, _)| p.seed))
        .unwrap_or(0);

    let (resolved_challenger, resolved_champion, bar_str) = validate_match_fields(
        id,
        eff_challenger.as_deref(),
        &eff_champion,
        eff_bar.as_deref(),
        eff_work,
        *cap,
        extends.is_some(),
        errors,
    );

    if let Some((ref p, parent_cap)) = parent {
        check_extends_agreement(
            id,
            p,
            parent_cap,
            *cap,
            &resolved_challenger,
            &resolved_champion,
            &bar_str,
            &eff_work_str,
            eff_work,
            eff_seed,
            errors,
        );
    }

    let root_id = parent
        .as_ref()
        .map_or_else(|| id.to_owned(), |(p, _)| p.root_id.clone());
    let resolved = ResolvedMatch {
        root_id,
        challenger: eff_challenger.unwrap_or_default(),
        champion: eff_champion,
        work: eff_work_str,
        bar: bar_str,
        cap: *cap,
        seed: eff_seed,
        extends: extends.clone(),
    };
    match_stages.insert(id, (resolved, *cap));
}

#[allow(clippy::too_many_arguments)]
fn validate_match_fields(
    id: &str,
    eff_challenger: Option<&str>,
    eff_champion: &str,
    eff_bar: Option<&str>,
    eff_work: u64,
    cap: u32,
    has_extends: bool,
    errors: &mut Vec<String>,
) -> (String, String, String) {
    let mut resolved_challenger = String::new();
    if let Some(c) = eff_challenger {
        match ladder::resolve(c, eff_work) {
            Ok(r) => resolved_challenger = r,
            Err(e) => errors.push(format!("stage {id}: challenger {c:?}: {e}")),
        }
    } else if !has_extends {
        errors.push(format!("stage {id}: missing challenger"));
    }

    let mut resolved_champion = String::new();
    match ladder::resolve(eff_champion, eff_work) {
        Ok(r) => resolved_champion = r,
        Err(e) => {
            errors.push(format!("stage {id}: champion {eff_champion:?}: {e}"));
        }
    }

    let bar_str = if let Some(b) = eff_bar {
        match Bar::named(b) {
            Some(Bar::Audit) if !cap.is_multiple_of(2) => {
                errors.push(format!(
                    "stage {id}: audit cap {cap} must be an even number"
                ));
            }
            Some(_) => {}
            None => errors.push(format!("stage {id}: unknown bar {b:?}")),
        }
        b.to_owned()
    } else {
        if !has_extends {
            errors.push(format!("stage {id}: missing bar"));
        }
        String::new()
    };

    if cap < 2 {
        errors.push(format!("stage {id}: cap {cap} must be at least 2"));
    }

    (resolved_challenger, resolved_champion, bar_str)
}

#[allow(clippy::too_many_arguments)]
fn check_extends_agreement(
    id: &str,
    p: &ResolvedMatch,
    parent_cap: u32,
    cap: u32,
    resolved_challenger: &str,
    resolved_champion: &str,
    bar_str: &str,
    eff_work_str: &str,
    eff_work: u64,
    eff_seed: u64,
    errors: &mut Vec<String>,
) {
    if cap < parent_cap {
        errors.push(format!(
            "stage {id}: cap {cap} is less than extended stage's cap {parent_cap}"
        ));
    }
    if !resolved_challenger.is_empty() {
        let parent_ch = ladder::resolve(&p.challenger, parse_work(&p.work).unwrap_or(PROXY))
            .unwrap_or_default();
        if !parent_ch.is_empty() && resolved_challenger != parent_ch {
            errors.push(format!(
                "stage {id}: challenger {resolved_challenger:?} differs from extended stage's {parent_ch:?}"
            ));
        }
    }
    if !resolved_champion.is_empty() {
        let parent_champ =
            ladder::resolve(&p.champion, parse_work(&p.work).unwrap_or(PROXY)).unwrap_or_default();
        if !parent_champ.is_empty() && resolved_champion != parent_champ {
            errors.push(format!(
                "stage {id}: champion {resolved_champion:?} differs from extended stage's {parent_champ:?}"
            ));
        }
    }
    if !bar_str.is_empty() && bar_str != p.bar {
        errors.push(format!(
            "stage {id}: bar {bar_str:?} differs from extended stage's {:?}",
            p.bar
        ));
    }
    if let Some(parent_work) = parse_work(&p.work)
        && eff_work != parent_work
    {
        errors.push(format!(
            "stage {id}: work {eff_work_str:?} differs from extended stage's {:?}",
            p.work
        ));
    }
    if eff_seed != p.seed {
        errors.push(format!(
            "stage {id}: seed {eff_seed} differs from extended stage's {}",
            p.seed
        ));
    }
}
