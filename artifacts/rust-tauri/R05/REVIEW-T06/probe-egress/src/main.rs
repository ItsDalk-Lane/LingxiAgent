//! R05-T06 R01 REVIEW probe: does the egress guard's literal-IP policy see
//! every host form the URL library canonicalizes to a guarded address?
//!
//! Method: run the REAL `EgressGuard::check_url` (production code from the
//! uncommitted working tree) on a battery of non-decimal IPv4 host spellings;
//! then parse the SAME strings with the `url` crate (the WHATWG parser
//! reqwest uses) and report the host it would actually dial. A row where the
//! guard PASSES and url canonicalizes to a guarded (loopback/private) IP is
//! a policy bypass at the dial level.

use lingxi_adapters::models::egress::EgressGuard;

fn guarded_url_host_ref(host: Option<url::Host<&str>>) -> String {
    match host {
        Some(url::Host::Ipv6(octets)) => format!("Ipv6 {octets}"),
        Some(url::Host::Domain(domain)) => format!("Domain({domain})"),
        None => "<none>".to_string(),
        Some(url::Host::Ipv4(_)) => unreachable!(),
    }
}

fn guarded_ip(octets: [u8; 4]) -> bool {
    let [a, b, _, _] = octets;
    a == 0
        || a == 10
        || a == 127
        || (a == 100 && (b & 0xc0) == 64)
        || (a == 169 && b == 254)
        || (a == 172 && (b & 0xf0) == 16)
        || (a == 192 && b == 168)
        || (a == 198 && (b & 0xfe) == 18)
}

/// True when the 16-byte v6 form is an IPv4-MAPPED address whose mapped v4
/// side is itself guarded (dual-stack sockets dial the mapped v4 address).
fn guarded_mapped_v6(octets: [u8; 16]) -> Option<[u8; 4]> {
    if octets[..10].iter().all(|b| *b == 0) && octets[10] == 0xff && octets[11] == 0xff {
        Some([octets[12], octets[13], octets[14], octets[15]])
    } else {
        None
    }
}

fn main() {
    // The same allowlist shape the production guard builds from provider
    // endpoints (a single public https origin, no loopback exception).
    let endpoints = vec!["https://api.example.test/v1".to_string()];
    let guard = EgressGuard::from_provider_endpoints(&endpoints).expect("guard builds");

    let battery = [
        // baseline: the dotted-decimal forms the guard DOES recognize
        "https://127.0.0.1:18080/asset.png",
        "https://10.1.2.3/asset.png",
        "https://192.168.3.5/asset.png",
        // non-decimal IPv4 spellings (WHATWG canonicalizes all of these)
        "https://0x7f.0.0.1/asset.png",      // hex octet
        "https://0x7f000001/asset.png",      // single hex number
        "https://2130706433/asset.png",      // single decimal number = 127.0.0.1
        "https://0177.0.0.1/asset.png",      // octal octet
        "https://0177.0.0.1:18080/asset.png", // octal octet with port
        "https://127.1/asset.png",           // short form = 127.0.0.1
        "https://127.000.000.001/asset.png", // leading zeros (guard's strict parser refuses; url?)
        "https://127.0.0.1./asset.png",      // trailing dot
        "https://3232235885/asset.png",      // decimal = 192.168.3.13? (probe prints actual)
        // bracketed IPv6 with a zone id (the guard refuses unparseable)
        "https://[fe80::1%25eth0]/asset.png",
        // IPv4-mapped IPv6 (dual-stack dials the mapped v4 address)
        "https://[::ffff:127.0.0.1]/asset.png",
        "https://[::ffff:169.254.169.254]/latest/meta-data",
        "https://[::ffff:10.1.2.3]/asset.png",
        "https://[::ffff:192.168.3.5]/asset.png",
    ];

    println!(
        "{:<44} | {:<9} | {:<28} | {}",
        "URL", "guard", "url-crate host", "verdict"
    );
    println!("{}", "-".repeat(110));
    let mut bypasses = 0usize;
    for candidate in battery {
        let guard_verdict = match guard.check_url(candidate) {
            Ok(()) => "PASS".to_string(),
            Err(err) => format!("REFUSE({:?})", err.code),
        };
        let url_side = match url::Url::parse(candidate) {
            Ok(parsed) => match parsed.host() {
                Some(url::Host::Ipv4(addr)) => {
                    let octets = addr.octets();
                    if guarded_ip(octets) {
                        format!("Ipv4 {addr} (GUARDED RANGE)")
                    } else {
                        format!("Ipv4 {addr}")
                    }
                }
                Some(url::Host::Ipv6(addr)) => {
                    let octets = addr.octets();
                    if let Some(mapped) = guarded_mapped_v6(octets) {
                        if guarded_ip(mapped) {
                            format!("Ipv6 {addr} (MAPPED->{mapped:?} GUARDED)")
                        } else {
                            format!("Ipv6 {addr} (mapped)")
                        }
                    } else {
                        format!("Ipv6 {addr}")
                    }
                }
                other => guarded_url_host_ref(other),
            },
            Err(err) => format!("parse-error {err}"),
        };
        let bypass = guard_verdict == "PASS"
            && (url_side.contains("GUARDED RANGE") || url_side.contains("MAPPED->"));
        if bypass {
            bypasses += 1;
        }
        println!(
            "{:<44} | {:<9} | {:<28} | {}",
            candidate,
            guard_verdict,
            url_side,
            if bypass { "!! BYPASS" } else { "ok" }
        );
    }
    println!("\nbypass rows: {bypasses}");
    // Exit non-zero only on infrastructure failure; the finding is reported
    // via the table itself.
}
