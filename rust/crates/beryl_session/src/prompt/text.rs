//! A prompt's state as text, for the hand-written expectations of tests (plan K24) and anyone
//! debugging. It says what the screen shows: the title, the shape's state, the marks and the
//! buttons. It leaves out the revision, which counts clicks.
//!
//! ```text
//! pick search: Search
//! tray #31 #32 #40
//! picked 1 of 0..2: #32
//! eligible #31 #40
//! selected #32
//! confirm[Find 1/2]* decline[Find Nothing]
//! ```
//!
//! The first line is the shape, the purpose and the title. Then the shape's own lines:
//!
//! ```text
//! tray #31 #32                      pick: the cards laid out, if any
//! picked 1 of 0..2: #32             pick: how many of how few to how many, and which
//! warned #5                         pick: those it asks about before it answers with them
//! order a2 a1 #7                    order: first to last
//! pile bottom "Bottom": #31         split: a pile, the top first
//! pile hand "Hand" exactly 2 closed: #32 #33
//! about #12                         yes or no, mode: the cards it is about, if any
//! number 2 of 0 1 2 3 ({2}{U})      number: the value, the values, what it means
//! catalog White | Blue* | Black     catalog: the entries, the chosen one starred
//! divide 5, 2 left: #4=2 #6=1       divide: the amount, what is left, each share
//! pay {3}{U}, {1} left              pay: the cost and what is still to pay
//! declare attack: #4>opponent #6>#20
//! aiming #8                         declare: the pairs, then those awaiting an aim
//! declare attack, no lines: #4>opponent       no line is drawn for a pair
//! ```
//!
//! Then a line for each mark in use, in the list's order, with what carries it: cards by slot,
//! then abilities, then players. Last the buttons, in their order: each is its id and its
//! label, in brackets when it can be clicked and in parentheses when not, with a star on the
//! primary one; a caption follows its label after a bar (`decline[Next | To Main 2]*`). An
//! answer offered as a button has its candidate's index for an id. A card is
//! `#` and its slot, an ability `a` and its number, a player `you` or `opponent`, as in a
//! script's text.

use std::fmt::{self, Display, Formatter, Write as _};

use super::{ButtonId, Marked, Pile, PromptButton, PromptView, ShapeView};
use crate::protocol::Side;
use crate::script::{Mark, Subject, wire};

/// Things separated by spaces, each after a space.
struct Each<I>(I);

impl<I: Iterator + Clone> Display for Each<I>
where
    I::Item: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.clone().try_for_each(|item| write!(f, " {item}"))
    }
}

/// A card by its slot.
struct Card(u16);

impl Display for Card {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

fn cards(slots: &[u16]) -> Each<impl Iterator<Item = Card> + Clone + '_> {
    Each(slots.iter().copied().map(Card))
}

/// Where a subject sorts in a mark's line: cards by slot, then abilities, then players.
fn rank(subject: Subject) -> (u8, u32) {
    match subject {
        Subject::Object { slot } => (0, slot.into()),
        Subject::Ability { id } => (1, id),
        Subject::Player { player } => (2, u32::from(player == Side::Opponent)),
    }
}

impl Display for PromptView {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let PromptView {
            shape,
            purpose,
            title,
            marks,
            buttons,
            state,
            ..
        } = self;
        writeln!(f, "{} {purpose}: {title}", wire(*shape))?;
        if let Some(state) = state {
            write!(f, "{state}")?;
        }
        for &mark in Mark::ALL {
            let mut on: Vec<Subject> = marks
                .iter()
                .filter(|m| m.mark == mark)
                .map(|&Marked { on, .. }| on)
                .collect();
            if on.is_empty() {
                continue;
            }
            on.sort_by_key(|&s| rank(s));
            writeln!(f, "{}{}", mark.name(), Each(on.iter()))?;
        }
        if !buttons.is_empty() {
            for (i, button) in buttons.iter().enumerate() {
                write!(f, "{}{button}", if i == 0 { "" } else { " " })?;
            }
            f.write_str("\n")?;
        }
        Ok(())
    }
}

impl Display for PromptButton {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (open, close) = if self.enabled { ('[', ']') } else { ('(', ')') };
        match self.id {
            ButtonId::Confirm => f.write_str("confirm")?,
            ButtonId::Decline => f.write_str("decline")?,
            ButtonId::Auto => f.write_str("auto")?,
            ButtonId::PassOn => f.write_str("passOn")?,
            ButtonId::Option { index } => write!(f, "{index}")?,
        }
        f.write_char(open)?;
        f.write_str(&self.label)?;
        if !self.caption.is_empty() {
            write!(f, " | {}", self.caption)?;
        }
        f.write_char(close)?;
        if self.primary {
            f.write_str("*")?;
        }
        Ok(())
    }
}

impl Display for ShapeView {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ShapeView::Pick {
                tray,
                least,
                most,
                picked,
                warned,
            } => {
                if !tray.is_empty() {
                    writeln!(f, "tray{}", cards(tray))?;
                }
                write!(f, "picked {} of ", picked.len())?;
                if least == most {
                    write!(f, "{most}")?;
                } else {
                    write!(f, "{least}..{most}")?;
                }
                if !picked.is_empty() {
                    write!(f, ":{}", Each(picked.iter()))?;
                }
                f.write_str("\n")?;
                if warned.is_empty() {
                    Ok(())
                } else {
                    writeln!(f, "warned{}", Each(warned.iter()))
                }
            }
            ShapeView::Order { items } => writeln!(f, "order{}", Each(items.iter())),
            ShapeView::Split { piles } => piles.iter().try_for_each(|pile| write!(f, "{pile}")),
            ShapeView::YesNo { about } | ShapeView::Mode { about } => {
                if about.is_empty() {
                    Ok(())
                } else {
                    writeln!(f, "about{}", cards(about))
                }
            }
            ShapeView::Number {
                values,
                value,
                detail,
            } => {
                write!(f, "number {value} of{}", Each(values.iter()))?;
                if let Some(detail) = detail {
                    write!(f, " ({detail})")?;
                }
                f.write_str("\n")
            }
            ShapeView::Catalog { entries, chosen } => {
                f.write_str("catalog")?;
                for (i, entry) in entries.iter().enumerate() {
                    let star = if *chosen == u16::try_from(i).ok() {
                        "*"
                    } else {
                        ""
                    };
                    write!(f, "{} {entry}{star}", if i == 0 { "" } else { " |" })?;
                }
                f.write_str("\n")
            }
            ShapeView::Divide {
                total,
                left,
                shares,
            } => {
                write!(f, "divide {total}, {left} left:")?;
                for share in shares {
                    write!(f, " {}={}", share.on, share.amount)?;
                }
                f.write_str("\n")
            }
            ShapeView::Pay { cost, left } => writeln!(f, "pay {cost}, {left} left"),
            ShapeView::Declare {
                arrow,
                pairs,
                lined,
                aiming,
            } => {
                let lines = if *lined { "" } else { ", no lines" };
                write!(f, "declare {}{lines}:", arrow.name())?;
                for pair in pairs {
                    write!(f, " #{}>{}", pair.from, pair.to)?;
                }
                f.write_str("\n")?;
                if aiming.is_empty() {
                    Ok(())
                } else {
                    writeln!(f, "aiming{}", cards(aiming))
                }
            }
        }
    }
}

impl Display for Pile {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "pile {} \"{}\"", wire(self.place), self.label)?;
        if let Some(n) = self.exactly {
            write!(f, " exactly {n}")?;
        }
        if self.closed {
            f.write_str(" closed")?;
        }
        writeln!(f, ":{}", cards(&self.cards))
    }
}
