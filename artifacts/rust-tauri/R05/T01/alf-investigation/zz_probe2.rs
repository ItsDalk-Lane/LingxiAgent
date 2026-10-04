// Bare ALF probe: zero Lingxi code, raw std::net only.
// Binds 0.0.0.0, then self-connects via loopback (control) and via the LAN address (test).
// If loopback succeeds but the LAN connection is never delivered to accept(),
// inbound to this adhoc-signed binary is being held by the macOS Application Firewall.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn main() {
    let lan_ip = std::env::args().nth(1).expect("usage: zz_probe2 <lan-ip>");
    let listener = TcpListener::bind("0.0.0.0:0").expect("bind 0.0.0.0");
    let port = listener.local_addr().unwrap().port();
    println!("probe pid={} listening 0.0.0.0:{port}", std::process::id());

    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("loopback connect");
    c.write_all(b"ping").expect("loopback write");
    let (mut s, from) = listener.accept().expect("loopback accept");
    let mut buf = [0u8; 4];
    s.read_exact(&mut buf).expect("loopback read");
    println!("LOOPBACK OK from {from}: {:?}", &buf);

    let addr = format!("{lan_ip}:{port}").parse().expect("lan addr");
    let mut c = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("lan connect");
    println!("LAN CONNECT OK (kernel-level handshake completed)");
    c.write_all(b"ping").expect("lan write into kernel buffer");
    println!("LAN WRITE OK (bytes accepted by kernel)");
    listener.set_nonblocking(true).expect("nonblocking");
    let start = Instant::now();
    loop {
        match listener.accept() {
            Ok((mut s, from)) => {
                let mut buf = [0u8; 4];
                s.read_exact(&mut buf).expect("lan read");
                println!("LAN ACCEPT+READ OK from {from}: {:?} — inbound NOT blocked", &buf);
                return;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() > Duration::from_secs(10) {
                    println!(
                        "LAN STALL: connect+write completed at kernel level but the connection \
                         never reached accept() in 10s — an application-layer firewall is holding \
                         inbound flows to this unsigned binary"
                    );
                    std::process::exit(2);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("accept error: {e}"),
        }
    }
}
