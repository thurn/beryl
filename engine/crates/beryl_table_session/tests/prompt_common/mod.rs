use std::path::Path;
use std::sync::mpsc::{Receiver, channel, sync_channel};

use beryl_link::memory;
use beryl_protocol::{GameUpdate, SessionIds};
use beryl_table::{Action, Seat};
use beryl_table_session::{HostSettings, TableSessions};

pub const HOST: Seat = Seat(0);
pub const GUEST: Seat = Seat(1);
const SECRET: &str = "3mxd";

pub struct Linked {
    pub host: TableSessions,
    pub guest: TableSessions,
    pub host_views: Receiver<GameUpdate>,
    pub guest_views: Receiver<GameUpdate>,
}

impl Linked {
    pub fn new(path: &Path, guest_card: &str, copies: u16) -> Self {
        let (host_emit, host_views) = channel();
        let (guest_emit, guest_views) = channel();
        let host = TableSessions::with_data_dir(SessionIds::default(), path, move |update| {
            let _ = host_emit.send(update);
        });
        let guest = TableSessions::new(SessionIds::default(), move |update| {
            let _ = guest_emit.send(update);
        });
        let (send, links) = sync_channel(1);
        let (host_link, guest_link) = memory::pair();
        host.host(
            links,
            HostSettings {
                name: "Host".into(),
                invite: format!("beryl-local:52114:{SECRET}"),
                secret: SECRET.into(),
                starting_life: 20,
                starting_hand_size: 7,
            },
        )
        .unwrap();
        host_views.recv().unwrap();
        send.send(host_link).unwrap();
        let mut guest_link = Some(guest_link);
        guest
            .join_checked(
                move || guest_link.take().ok_or(beryl_link::LinkError::Closed),
                "Guest".into(),
                SECRET.into(),
            )
            .unwrap();
        let game = Self {
            host,
            guest,
            host_views,
            guest_views,
        };
        game.updates();
        for (session, seat, name) in [
            (&game.host, HOST, "Island"),
            (&game.guest, GUEST, guest_card),
        ] {
            session
                .act(Action::LoadDeck {
                    seat,
                    main: vec![(name.into(), copies)],
                    side: vec![],
                })
                .unwrap();
            game.updates();
        }
        game.host
            .act(Action::Start {
                first: HOST,
                opening: false,
            })
            .unwrap();
        game.updates();
        game
    }

    pub fn updates(&self) -> (GameUpdate, GameUpdate) {
        (
            self.host_views.recv().unwrap(),
            self.guest_views.recv().unwrap(),
        )
    }
}
