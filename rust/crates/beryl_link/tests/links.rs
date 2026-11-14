use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::thread;

use beryl_link::{Invite, Link, LinkError, MAX_FRAME_SIZE, Route, TcpHost, TcpLink, memory};

fn tcp_pair() -> (TcpLink, TcpLink) {
    let (host, invite) = TcpHost::listen().unwrap();
    let guest = TcpLink::connect(&invite).unwrap();
    (host.accept().unwrap(), guest)
}

fn exchange_frames(mut first: impl Link + 'static, mut second: impl Link, route: Route) {
    assert_eq!(first.peer().route, route);
    assert_eq!(second.peer().route, route);
    let sender = thread::spawn(move || {
        first.send(&[]).unwrap();
        for number in 0..1000_u32 {
            first.send(&number.to_be_bytes()).unwrap();
        }
        assert_eq!(first.receive().unwrap(), b"received");
    });
    assert!(second.receive().unwrap().is_empty());
    for number in 0..1000_u32 {
        assert_eq!(second.receive().unwrap(), number.to_be_bytes());
    }
    second.send(b"received").unwrap();
    sender.join().unwrap();
    assert_eq!(second.receive(), Err(LinkError::Closed));
}

#[test]
fn memory_delivers_frames_in_order_in_both_directions() {
    let (first, second) = memory::pair();
    exchange_frames(first, second, Route::Memory);
}

#[test]
fn tcp_delivers_frames_in_order_in_both_directions() {
    let (first, second) = tcp_pair();
    exchange_frames(first, second, Route::Local);
}

#[test]
fn a_reopened_tcp_listener_uses_the_original_invite() {
    let (host, invite) = TcpHost::listen().unwrap();
    let guest = TcpLink::connect(&invite).unwrap();
    let mut link = host.accept().unwrap();
    // Close from the guest so its ephemeral port owns the TCP TIME_WAIT state.
    drop(guest);
    assert_eq!(link.receive(), Err(LinkError::Closed));
    drop(link);
    drop(host);

    let host = TcpHost::resume(&invite).unwrap();
    let mut guest = TcpLink::connect(&invite).unwrap();
    let mut link = host.accept().unwrap();
    guest.send(b"hello again").unwrap();
    assert_eq!(link.receive().unwrap(), b"hello again");
}

#[test]
fn sever_closes_memory_in_both_directions() {
    let (mut first, mut second) = memory::pair();
    let mut sender = first.try_clone_sender().unwrap();
    first.send(b"queued").unwrap();
    first.sever();
    assert_eq!(second.receive().unwrap(), b"queued");
    assert_eq!(second.receive(), Err(LinkError::Closed));
    assert_eq!(first.receive(), Err(LinkError::Closed));
    assert_eq!(sender.send(b"late"), Err(LinkError::Closed));
}

fn send_while_receiving(mut first: impl Link + 'static, mut second: impl Link) {
    let mut sender = first.try_clone_sender().unwrap();
    let receiver = thread::spawn(move || {
        // The peer waits for the handle's frame before sending anything.
        assert_eq!(first.receive().unwrap(), b"reply");
        first.send(b"after dropping the sender").unwrap();
        assert_eq!(first.receive(), Err(LinkError::Closed));
    });
    sender.send(b"request").unwrap();
    assert_eq!(second.receive().unwrap(), b"request");
    drop(sender);
    second.send(b"reply").unwrap();
    assert_eq!(second.receive().unwrap(), b"after dropping the sender");
    drop(second);
    receiver.join().unwrap();
}

#[test]
fn memory_sender_can_send_while_receiving() {
    let (first, second) = memory::pair();
    send_while_receiving(first, second);
}

#[test]
fn tcp_sender_can_send_while_receiving() {
    let (first, second) = tcp_pair();
    send_while_receiving(first, second);
}

fn close_through_sender(mut first: impl Link + 'static, mut second: impl Link) {
    let sender = first.try_clone_sender().unwrap();
    let receiver = thread::spawn(move || {
        assert_eq!(first.receive(), Err(LinkError::Closed));
    });
    sender.close().unwrap();
    assert_eq!(second.receive(), Err(LinkError::Closed));
    receiver.join().unwrap();
}

#[test]
fn closing_a_memory_sender_disconnects_both_ends() {
    let (first, second) = memory::pair();
    close_through_sender(first, second);
}

#[test]
fn closing_a_tcp_sender_disconnects_both_ends() {
    let (first, second) = tcp_pair();
    close_through_sender(first, second);
}

fn refuse_oversized_send(mut first: impl Link, mut second: impl Link, oversized: &[u8]) {
    assert!(matches!(first.send(oversized), Err(LinkError::Refused(_))));
    first.send(b"valid").unwrap();
    assert_eq!(second.receive().unwrap(), b"valid");
}

#[test]
fn links_refuse_oversized_sends_before_writing() {
    let oversized = vec![0; MAX_FRAME_SIZE + 1];
    let (first, second) = memory::pair();
    refuse_oversized_send(first, second, &oversized);
    let (first, second) = tcp_pair();
    refuse_oversized_send(first, second, &oversized);
}

#[test]
fn tcp_uses_big_endian_lengths_and_refuses_oversized_input() {
    let (host, invite) = TcpHost::listen().unwrap();
    let Invite::Local { port, .. } = invite else {
        panic!("expected a local invite");
    };
    let mut raw = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let mut link = host.accept().unwrap();
    raw.write_all(b"\0\0\0\x03abc").unwrap();
    assert_eq!(link.receive().unwrap(), b"abc");
    link.send(b"test").unwrap();
    let mut reply = [0; 8];
    raw.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"\0\0\0\x04test");

    raw.write_all(&(MAX_FRAME_SIZE as u32 + 1).to_be_bytes())
        .unwrap();
    assert!(matches!(link.receive(), Err(LinkError::Refused(_))));
    assert_eq!(link.receive(), Err(LinkError::Closed));
}

#[test]
fn invites_round_trip_as_text() {
    let local = "beryl-local:52114:3mxd";
    let invite = local.parse::<Invite>().unwrap();
    assert_eq!(
        invite,
        Invite::Local {
            port: 52114,
            secret: "3mxd".into()
        }
    );
    assert_eq!(invite.to_string(), local);

    let remote = "beryl:k7fq3mxdpw2a91hc";
    let invite = remote.parse::<Invite>().unwrap();
    assert_eq!(invite, Invite::Remote(remote.into()));
    assert_eq!(invite.to_string(), remote);

    let (_, invite) = TcpHost::listen().unwrap();
    assert_eq!(invite.to_string().parse::<Invite>().unwrap(), invite);
    let Invite::Local { secret, .. } = invite else {
        panic!("expected a local invite");
    };
    assert_eq!(secret.len(), 8);
}

#[test]
fn malformed_local_invites_are_refused() {
    for code in [
        "",
        "other:52114:3mxd",
        "beryl-local:52114",
        "beryl-local:0:3mxd",
        "beryl-local:65536:3mxd",
        "beryl-local:+1:3mxd",
        "beryl-local:52114:",
        "beryl-local:52114:3mxd:extra",
    ] {
        assert!(
            matches!(code.parse::<Invite>(), Err(LinkError::Refused(_))),
            "{code:?}"
        );
    }
}
