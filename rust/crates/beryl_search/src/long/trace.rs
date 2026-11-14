//! The first four decisions below a root choice, grouped by what each world showed.

use std::collections::BTreeMap;

use super::{
    Choice, Decision, DecisionKind, GameTable, Layout, LongSearch, Observation, Seat, State, Tree,
};

const LEVELS: usize = 4;

/// The ordinary search's choice and the observed branches for the two requested moves.
pub struct TreeReport {
    pub choice: Choice,
    pub branches: [Branch; 2],
    pub turn: Option<super::TurnReport>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Visits {
    pub count: u64,
    reward: f64,
}

impl Visits {
    /// The visit-weighted mean score for the player at the root.
    pub fn mean(self) -> Option<f64> {
        (self.count > 0).then(|| self.reward / self.count as f64)
    }

    fn add(&mut self, other: Self) {
        self.count += other.count;
        self.reward += other.reward;
    }
}

pub struct Branch {
    pub visits: Visits,
    /// Each successive level follows the most visited child of the preceding level.
    pub levels: Vec<Level>,
}

pub struct Level {
    /// A resampling tree can reach several different decisions below a move.
    pub decisions: Vec<ReachedDecision>,
    /// The most visited child as (decision index, child index).
    pub follow: Option<(usize, usize)>,
}

pub struct ReachedDecision {
    pub player: Seat,
    pub kind: DecisionKind,
    pub trees: usize,
    pub children: Vec<(String, Visits)>,
}

type Labels<L> = fn(&GameTable, &State<L>, &Decision<L>) -> Vec<String>;

pub(super) struct Capture<L: Layout> {
    choices: [Choice; 2],
    labels: Labels<L>,
    branches: [Branch; 2],
}

impl<L: Layout> LongSearch<L> {
    /// Chooses as usual, also reporting the top of the two requested branches.
    /// `labels` names candidates in the world where the search encountered them.
    /// Reports visits made by this call, so use a fresh agent for a whole-position report.
    pub fn choose_with_tree(
        &mut self,
        obs: Observation<'_, L>,
        d: &Decision<L>,
        choices: [Choice; 2],
        labels: Labels<L>,
    ) -> TreeReport {
        let mut capture = Capture {
            choices,
            labels,
            branches: choices.map(|_| Branch {
                visits: Visits::default(),
                levels: Vec::new(),
            }),
        };
        let choice = self.choose_traced(obs, d, self.decision, Some(&mut capture));
        TreeReport {
            choice,
            branches: capture.branches,
            turn: self.last.as_ref().and_then(|search| search.turn),
        }
    }
}

impl<L: Layout> Capture<L> {
    pub(super) fn finish_turn(&mut self, search: &crate::Search) {
        for (choice, branch) in self.choices.into_iter().zip(&mut self.branches) {
            let i = usize::from(choice.0);
            branch.visits = Visits {
                count: u64::from(search.visits[i]),
                reward: search.rewards[i],
            };
        }
    }

    pub(super) fn start(&self, trees: &mut [Tree<L>]) {
        for tree in trees {
            tree.trace = Some(Box::new(Trace {
                choices: self.choices,
                labels: self.labels,
                moves: vec![Move::default(), Move::default()],
                path: Vec::with_capacity(1 + LEVELS),
            }));
        }
    }

    pub(super) fn finish(&mut self, trees: &mut [Tree<L>]) {
        let traces: Vec<_> = trees
            .iter_mut()
            .filter_map(|tree| tree.trace.take())
            .collect();
        self.branches = self.choices.map(|choice| {
            let roots = traces
                .iter()
                .filter_map(|trace| Some((trace.moves.as_slice(), trace.root(choice)?)))
                .collect();
            branch(roots)
        });
    }
}

#[derive(Debug)]
pub(super) struct Trace<L: Layout> {
    choices: [Choice; 2],
    labels: Labels<L>,
    moves: Vec<Move>,
    path: Vec<usize>,
}

#[derive(Debug, Default)]
struct Move {
    visits: Visits,
    decisions: Vec<DecisionTrace>,
}

#[derive(Debug)]
struct DecisionTrace {
    player: Seat,
    kind: DecisionKind,
    children: BTreeMap<String, usize>,
}

impl<L: Layout> Trace<L> {
    fn root(&self, choice: Choice) -> Option<usize> {
        self.choices.iter().position(|&c| c == choice)
    }

    pub(super) fn enter(&mut self, choice: Choice) {
        self.path.clear();
        self.path.extend(self.root(choice));
    }

    pub(super) fn advance(
        &mut self,
        table: &GameTable,
        state: &State<L>,
        d: &Decision<L>,
        choice: Choice,
        policy_only: bool,
    ) {
        let Some(&parent) = self.path.last().filter(|_| self.path.len() <= LEVELS) else {
            return;
        };
        let mut next = self.moves.len();
        let decisions = &mut self.moves[parent].decisions;
        let i = decisions
            .iter()
            .position(|g| g.player == d.player && g.kind == d.kind);
        let group = i.unwrap_or_else(|| {
            decisions.push(DecisionTrace {
                player: d.player,
                kind: d.kind,
                children: BTreeMap::new(),
            });
            decisions.len() - 1
        });
        for (i, label) in (self.labels)(table, state, d).into_iter().enumerate() {
            let selected = i == usize::from(choice.0);
            if !policy_only || selected {
                let child = *decisions[group].children.entry(label).or_insert_with(|| {
                    let index = next;
                    next += 1;
                    index
                });
                if selected {
                    self.path.push(child);
                }
            }
        }
        self.moves.resize_with(next, Move::default);
    }

    pub(super) fn score(&mut self, reward: f64) {
        for &i in &self.path {
            self.moves[i].visits.add(Visits { count: 1, reward });
        }
    }
}

type NodeRef<'a> = (&'a [Move], usize);

struct Group<'a> {
    player: Seat,
    kind: DecisionKind,
    trees: usize,
    children: BTreeMap<&'a str, (Visits, Vec<NodeRef<'a>>)>,
}

fn branch(mut nodes: Vec<NodeRef<'_>>) -> Branch {
    let mut visits = Visits::default();
    for &(moves, i) in &nodes {
        visits.add(moves[i].visits);
    }
    let mut levels = Vec::new();
    while !nodes.is_empty() && levels.len() < LEVELS {
        let groups = groups(&nodes);
        if groups.is_empty() {
            break;
        }
        let mut best = 0;
        let mut follow = None;
        let mut next = Vec::new();
        let decisions = groups
            .into_iter()
            .enumerate()
            .map(|(g, group)| ReachedDecision {
                player: group.player,
                kind: group.kind,
                trees: group.trees,
                children: group
                    .children
                    .into_iter()
                    .enumerate()
                    .map(|(c, (label, (visits, nodes)))| {
                        if visits.count > best {
                            best = visits.count;
                            follow = Some((g, c));
                            next = nodes;
                        }
                        (label.to_owned(), visits)
                    })
                    .collect(),
            })
            .collect();
        levels.push(Level { decisions, follow });
        nodes = next;
    }
    Branch { visits, levels }
}

fn groups<'a>(nodes: &[NodeRef<'a>]) -> Vec<Group<'a>> {
    let mut groups: Vec<Group<'_>> = Vec::new();
    // Each tree contributes one node at most: it already grouped moves by decision and label.
    for &(moves, i) in nodes {
        for decision in &moves[i].decisions {
            let group = if let Some(group) = groups
                .iter_mut()
                .find(|g| g.player == decision.player && g.kind == decision.kind)
            {
                group
            } else {
                groups.push(Group {
                    player: decision.player,
                    kind: decision.kind,
                    trees: 0,
                    children: BTreeMap::new(),
                });
                groups.last_mut().unwrap()
            };
            group.trees += 1;
            for (label, &child) in &decision.children {
                let (visits, nodes) = group.children.entry(label).or_default();
                visits.add(moves[child].visits);
                nodes.push((moves, child));
            }
        }
    }
    groups
}
