// REVIEW-T01 independent ALF probe — written by the reviewer, zero Lingxi code.
// Semantics: bind a wildcard socket; prove loopback delivery works (control);
// then self-connect via the machine's LAN address. If the TCP handshake and
// payload write complete at kernel level yet accept() never returns the
// connection, inbound flows to this exact binary are being held above the
// network stack (macOS Application Firewall for a freshly-linked adhoc binary).
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn main() {
    let lan = std::env::args()
        .nth(1)
        .expect("usage: zz_alfprobe_r1 <lan-ip>");
    let srv = TcpListener::bind("0.0.0.0:0").expect("bind wildcard");
    let port = srv.local_addr().unwrap().port();
    println!("pid={} port={}", std::process::id(), port);

    // Control: loopback path must work end to end.
    let mut lc = TcpStream::connect(("127.0.0.1", port)).expect("loopback dial");
    lc.write_all(b"PING").expect("loopback send");
    let (mut ls, peer) = srv.accept().expect("loopback accept");
    let mut b = [0u8; 4];
    ls.read_exact(&mut b).expect("loopback recv");
    println!("LOOPBACK PATH DELIVERED from {peer} payload={:?}", &b);

    // Test: same self-connection, but addressed to the LAN interface.
    let dst = format!("{lan}:{port}").parse().expect("lan socket addr");
    let mut wc = TcpStream::connect_timeout(&dst, Duration::from_secs(5)).expect("lan dial");
    println!("LAN HANDSHAKE COMPLETE");
    wc.write_all(b"PING").expect("lan send");
    println!("LAN PAYLOAD ACCEPTED BY KERNEL");

    srv.set_nonblocking(true).expect("nonblocking accept");
    let t0 = Instant::now();
    loop {
        match srv.accept() {
            Ok((mut ws, peer)) => {
                let mut b = [0u8; 4];
                ws.read_exact(&mut b).expect("lan recv");
                println!("LAN DELIVERY OK from {peer} payload={:?} — inbound NOT held", &b);
                return;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if t0.elapsed() > Duration::from_secs(12) {
                    println!(
                        "LAN STALL AFTER 12s: handshake+payload completed below the \
                         application, but accept() never saw the connection — an \
                         application-layer firewall is holding inbound flows to this \
                         freshly-linked adhoc-signed binary"
                    );
                    std::process::exit(2);
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            Err(e) => panic!("accept failure: {e}"),
        }
    }
}
