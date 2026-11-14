use std::{sync::Mutex, thread};

use beryl_link::{
    Invite, Link, LinkError, MAX_FRAME_SIZE, PING_INTERVAL, Route,
    guest::{Event, REDIAL_DELAY, join},
    host::{Host, Hosted, serve},
    wire::{Hello, RefusalReason},
};
use beryl_link_iroh::{Identity, IrohHost, IrohLink, RelayChoice, RemoteInvite};
use data_encoding::BASE32_NOPAD;
use serde_json::Value;

#[test]
fn exchanges_ordered_frames_and_drains_before_close() {
    let directory = tempfile::tempdir().unwrap();
    let host_key = Identity::open(directory.path().join("host")).unwrap();
    let guest_key = Identity::open(directory.path().join("guest")).unwrap();
    let (host, invite) = IrohHost::open(host_key.clone(), RelayChoice::Disabled).unwrap();
    let code = invite.to_string();
    let text = code.strip_prefix("beryl:").unwrap();
    assert!(
        text.bytes()
            .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(&byte))
    );
    let invite: Invite = code.parse().unwrap();
    let decoded = RemoteInvite::decode(&invite).unwrap();
    assert_eq!(decoded.endpoint_id(), host_key.endpoint_id());
    assert_eq!(decoded.relay(), None);
    assert_eq!(decoded.encode(), invite);

    let (mut guest, mut link) = thread::scope(|scope| {
        let accepting = scope.spawn(|| host.accept().unwrap());
        let guest = IrohLink::connect(guest_key, &invite, RelayChoice::Disabled).unwrap();
        (guest, accepting.join().unwrap())
    });
    let mut sender = guest.try_clone_sender().unwrap();
    drop(host);
    assert!(matches!(
        guest.send(&vec![0; MAX_FRAME_SIZE + 1]),
        Err(LinkError::Refused(_))
    ));
    guest.send(b"first").unwrap();
    sender.send(b"second").unwrap();
    assert_eq!(link.receive().unwrap(), b"first");
    assert_eq!(link.receive().unwrap(), b"second");
    assert_eq!(link.peer().route, Route::Direct);
    assert!(link.peer().round_trip.is_some());
    link.send(b"reply").unwrap();
    link.send(b"last").unwrap();
    drop(link);
    assert_eq!(guest.receive().unwrap(), b"reply");
    assert_eq!(guest.receive().unwrap(), b"last");
    assert_eq!(guest.receive(), Err(LinkError::Closed));
}

#[test]
fn a_reopened_iroh_listener_uses_the_original_invite() {
    let directory = tempfile::tempdir().unwrap();
    let host_key = Identity::open(directory.path().join("host")).unwrap();
    let guest_key = Identity::open(directory.path().join("guest")).unwrap();
    let (host, invite) = IrohHost::open(host_key.clone(), RelayChoice::Disabled).unwrap();
    assert!(matches!(
        IrohHost::resume(guest_key.clone(), &invite),
        Err(LinkError::Refused(_))
    ));
    host.close().unwrap();
    let host = IrohHost::resume(host_key, &invite).unwrap();
    thread::scope(|scope| {
        let accepting = scope.spawn(|| host.accept().unwrap());
        let mut guest = IrohLink::connect(guest_key, &invite, RelayChoice::Disabled).unwrap();
        guest.send(b"hello again").unwrap();
        assert_eq!(accepting.join().unwrap().receive().unwrap(), b"hello again");
    });
}

#[test]
fn sender_closes_both_ends_while_receiving() {
    let directory = tempfile::tempdir().unwrap();
    let host_key = Identity::open(directory.path().join("host")).unwrap();
    let guest_key = Identity::open(directory.path().join("guest")).unwrap();
    let (host, invite) = IrohHost::open(host_key, RelayChoice::Disabled).unwrap();
    let (mut guest, mut link) = thread::scope(|scope| {
        let accepting = scope.spawn(|| host.accept().unwrap());
        let guest = IrohLink::connect(guest_key, &invite, RelayChoice::Disabled).unwrap();
        (guest, accepting.join().unwrap())
    });
    let mut sender = link.try_clone_sender().unwrap();
    thread::scope(|scope| {
        let waiting = scope.spawn(move || {
            link.send(b"ready").unwrap();
            link.receive()
        });
        assert_eq!(guest.receive().unwrap(), b"ready");
        sender.close().unwrap();
        assert_eq!(waiting.join().unwrap(), Err(LinkError::Closed));
        assert_eq!(sender.send(b"late"), Err(LinkError::Closed));
        assert_eq!(guest.receive(), Err(LinkError::Closed));
    });
}

#[test]
fn refuses_malformed_invites_and_wrong_table_secret() {
    let directory = tempfile::tempdir().unwrap();
    let host_key = Identity::open(directory.path().join("host")).unwrap();
    let guest_key = Identity::open(directory.path().join("guest")).unwrap();
    for code in [
        "beryl:",
        "beryl:bad!=",
        "beryl:aaaaaaaa",
        "beryl-local:42:secret",
    ] {
        let invite = code.parse().unwrap();
        assert!(matches!(
            IrohLink::connect(guest_key.clone(), &invite, RelayChoice::Disabled),
            Err(LinkError::Refused(_))
        ));
    }

    let (host, invite) = IrohHost::open(host_key, RelayChoice::Disabled).unwrap();
    let secret = RemoteInvite::decode(&invite).unwrap().secret();
    let mut bytes = BASE32_NOPAD
        .decode(invite.to_string()[6..].to_ascii_uppercase().as_bytes())
        .unwrap();
    // Change one byte of the secret while keeping the same authenticated endpoint.
    bytes[33] ^= 1;
    let wrong_invite = Invite::Remote(format!(
        "beryl:{}",
        BASE32_NOPAD.encode(&bytes).to_ascii_lowercase()
    ));
    let hello = Hello {
        wire: "test".into(),
        secret: RemoteInvite::decode(&wrong_invite).unwrap().secret(),
        name: "Guest".into(),
    };
    let mut guest = join(
        move || IrohLink::connect(guest_key.clone(), &wrong_invite, RelayChoice::Disabled),
        hello,
        REDIAL_DELAY,
        PING_INTERVAL,
    );
    let events = guest.take_events().unwrap();

    let result = serve(
        host.accept().unwrap(),
        &mut Host::new("test", secret, "Host"),
        &Mutex::new(NoTableData),
        PING_INTERVAL,
    );
    assert_eq!(
        result,
        Err(LinkError::Refused("The invite secret is incorrect.".into()))
    );
    assert_eq!(events.recv().unwrap(), Event::Failed(LinkError::Closed));
}

struct NoTableData;

impl Hosted for NoTableData {
    fn seat_for(&mut self, _: &str) -> Option<u8> {
        panic!("The wrong invite reached the table")
    }
    fn act(&mut self, _: u8, _: Value) -> Result<(), RefusalReason> {
        unreachable!()
    }
    fn update_for(&mut self, _: u8) -> Value {
        unreachable!()
    }
    fn lobby_for(&mut self, _: u8) -> Option<Value> {
        unreachable!()
    }
    fn left(&mut self, _: u8) {
        unreachable!()
    }
    fn dropped(&mut self, _: u8) {
        unreachable!()
    }
}

#[test]
fn decodes_a_relay_address_and_preserves_the_invite() {
    let directory = tempfile::tempdir().unwrap();
    let identity = Identity::open(directory.path().join("host")).unwrap();
    let mut bytes = vec![1];
    bytes.extend_from_slice(identity.endpoint_id().as_bytes());
    bytes.extend_from_slice(&[0xff; 16]);
    bytes.push(1);
    bytes.extend_from_slice(b"https://relay.example.com/");
    let invite = Invite::Remote(format!(
        "beryl:{}",
        BASE32_NOPAD.encode(&bytes).to_ascii_lowercase()
    ));
    let decoded = RemoteInvite::decode(&invite).unwrap();
    assert_eq!(decoded.endpoint_id(), identity.endpoint_id());
    assert_eq!(
        decoded.relay().unwrap().as_str(),
        "https://relay.example.com/"
    );
    assert_eq!(decoded.secret(), "77777777777777777777777774");
    assert_eq!(decoded.encode(), invite);
}

#[test]
fn concurrent_opens_share_one_private_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("identity");
    let (first, second) = thread::scope(|scope| {
        let other = scope.spawn(|| Identity::open(&path).unwrap().endpoint_id());
        (
            Identity::open(&path).unwrap().endpoint_id(),
            other.join().unwrap(),
        )
    });
    assert_eq!(first, second);
    assert_eq!(Identity::open(&path).unwrap().endpoint_id(), first);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o077, 0);
    }
    std::fs::write(&path, [0; 33]).unwrap();
    assert!(matches!(Identity::open(&path), Err(LinkError::Io(_))));
}
