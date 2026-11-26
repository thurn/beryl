use beryl_protocol::protocol::{ManaPool, SharedPiles, Side};
use beryl_protocol::script::{Act, Edge, Table as Played, Tally};
use beryl_protocol::testing::{ViewBuilder, timing};
use game_play::sound::{Cue, Found, cues, id};

#[test]
fn shared_table_tally_uses_player_life_and_mana() {
    let mut view = ViewBuilder::sample().build();
    view.shared = Some(SharedPiles::default());
    view.players.you.life = 20;
    view.players.you.mana = ManaPool {
        white: 2,
        ..Default::default()
    };

    let table = Played::of(&view);
    assert!(table.shared.is_some());
    let timing = timing(0.0);
    let found = Found {
        table: &table,
        earlier: &[],
        timing: &timing,
    };

    let life_loss: Vec<Cue> = cues(
        &Act::Count {
            tally: Tally::Life {
                player: Side::You,
                life: 18,
            },
        },
        &found,
    )
    .collect();
    assert_eq!(
        life_loss,
        vec![Cue {
            sound: id("drain"),
            edge: Edge::Start,
            into: None,
        }]
    );

    let mana_spend: Vec<Cue> = cues(
        &Act::Count {
            tally: Tally::Pool {
                player: Side::You,
                mana: ManaPool {
                    white: 1,
                    ..Default::default()
                },
            },
        },
        &found,
    )
    .collect();
    assert!(mana_spend.is_empty());
}
