//! The tip round's part of `ignite status`: the last round any machine recorded, and where the
//! green branch is.

use std::sync::Arc;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use super::StatusSection;

/// Where the green branch is: its commit and how many commits the `staging` tip is ahead of it.
pub type GreenAt = Arc<dyn Fn() -> Result<(Commit, usize)> + Send + Sync>;

/// The last tip verification round any machine recorded at the hub: its tip, machine and where
/// its time went. With `repository.green` set, a second line says where `green` is.
pub struct TipRoundSection {
    pub hub: Arc<dyn ignite_core::traits::Hub>,
    /// `None` when `repository.green` is unset.
    pub green: Option<GreenAt>,
}

impl StatusSection for TipRoundSection {
    fn title(&self) -> &'static str {
        "tip round"
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = vec![match self.hub.last_tip_round() {
            Ok(Some(round)) => {
                format!("{} on {}: {}", round.commit, round.machine, round.summary())
            }
            Ok(None) => "none recorded".to_owned(),
            Err(error) => format!("the hub did not answer: {error}"),
        }];
        if let Some(green) = &self.green {
            lines.push(green_line(green()));
        }
        lines
    }
}

/// `ignite status`'s line for the green branch.
fn green_line(at: Result<(Commit, usize)>) -> String {
    match at {
        Ok((commit, 0)) => format!("green at {commit}, the tip"),
        Ok((commit, 1)) => format!("green at {commit}, 1 commit behind the tip"),
        Ok((commit, behind)) => format!("green at {commit}, {behind} commits behind the tip"),
        Err(error) => format!("green not read: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use ignite_core::fakes::FakeHub;
    use ignite_core::traits::Error;

    use super::*;

    fn section(green: Option<GreenAt>) -> Vec<String> {
        TipRoundSection {
            hub: Arc::new(FakeHub::default()),
            green,
        }
        .lines()
    }

    #[test]
    fn status_says_where_green_is_beside_the_tip_round() {
        let at = Commit::new("abc1234").unwrap();
        assert_eq!(section(None), ["none recorded"]);
        let behind: GreenAt = Arc::new(move || Ok((at.clone(), 3)));
        assert_eq!(
            section(Some(behind)),
            [
                "none recorded",
                "green at abc1234, 3 commits behind the tip"
            ]
        );
        let missing: GreenAt = Arc::new(|| Err(Error::NotFound("green".to_owned())));
        assert!(section(Some(missing))[1].starts_with("green not read: "));
    }
}
