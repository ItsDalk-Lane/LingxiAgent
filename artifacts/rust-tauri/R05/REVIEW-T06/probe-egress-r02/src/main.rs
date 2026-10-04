//! R05-T06 R02 REVIEW probe: re-verify the fix-r1 egress guard against the
//! R01 battery PLUS rows the fixer's new unit tests did NOT pin (single
//! tiny integers, hex short forms, uppercase spellings, verbose mapped v6,
//! v4-compatible, ULA/multicast v6, userinfo traps, a public v6, and a
//! numeric-prefix DOMAIN that is legitimately DNS scope).
//!
//! Method (same as R01): run the REAL `EgressGuard::check_url` (production
//! code from the uncommitted working tree) on every row, then parse the
//! SAME string with the `url` crate (the WHATWG parser reqwest dials with,
//! 2.5.8 = the workspace lock's version) and compare. A row where the
//! guard PASSES and the url side canonicalizes to a GUARDED address is a
//! policy bypass.

use lingxi_adapters::models::egress::EgressGuard;

fn guarded_v4(octets: [u8; 4]) -> bool {
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

/// The v6 policy the registered §29.11 text demands (mirrors production):
/// mapped ::ffff:0:0/96 folds to v4; ::/96 refuses; ULA/fe80/multicast
/// refuse; everything else is public.
fn guarded_v6_label(octets: [u8; 16]) -> Option<String> {
    if octets[..10].iter().all(|b| *b == 0) && octets[10] == 0xff && octets[11] == 0xff {
        let mapped = [octets[12], octets[13], octets[14], octets[15]];
        return if guarded_v4(mapped) {
            Some(format!("MAPPED->{mapped:?} GUARDED"))
        } else {
            None
        };
    }
    if octets[..12].iter().all(|b| *b == 0) {
        return Some("V4-COMPAT/UNSPECIFIED GUARDED".to_string());
    }
    let first = octets[0];
    if (first & 0xfe) == 0xfc {
        return Some("ULA GUARDED".to_string());
    }
    if first == 0xfe && (octets[1] & 0xc0) == 0x80 {
        return Some("LINK-LOCAL GUARDED".to_string());
    }
    if first == 0xff {
        return Some("MULTICAST GUARDED".to_string());
    }
    None
}

fn main() {
    let endpoints = vec!["https://api.example.test/v1".to_string()];
    let guard = EgressGuard::from_provider_endpoints(&endpoints).expect("guard builds");

    let battery = [
        // ── R01 battery (13 former BYPASS rows + baselines), unchanged ──
        "https://127.0.0.1:18080/asset.png",
        "https://10.1.2.3/asset.png",
        "https://192.168.3.5/asset.png",
        "https://0x7f.0.0.1/asset.png",
        "https://0x7f000001/asset.png",
        "https://2130706433/asset.png",
        "https://0177.0.0.1/asset.png",
        "https://0177.0.0.1:18080/asset.png",
        "https://127.1/asset.png",
        "https://127.000.000.001/asset.png",
        "https://127.0.0.1./asset.png",
        "https://3232235885/asset.png",
        "https://[fe80::1%25eth0]/asset.png",
        "https://[::ffff:127.0.0.1]/asset.png",
        "https://[::ffff:169.254.169.254]/latest/meta-data",
        "https://[::ffff:10.1.2.3]/asset.png",
        "https://[::ffff:192.168.3.5]/asset.png",
        // ── R02 rows NOT pinned by the fixer's new unit tests ──
        "https://0/asset.png",                          // single 0 = 0.0.0.0
        "https://1/asset.png",                          // 0.0.0.1 (0/8 guarded)
        "https://0x7f.1/asset.png",                     // hex short form = 127.0.0.1
        "https://017700000001/asset.png",               // octal single integer
        "https://017700000001:8443/asset.png",          // octal integer + port
        "HTTPS://0X7F.0.0.1/asset.png",                 // uppercase scheme + hex
        "https://[::ffff:127.0.0.1]:8443/asset.png",    // mapped v6 + port
        "https://[0:0:0:0:0:ffff:10.0.0.1]/asset.png",  // verbose mapped private
        "https://[::0.0.0.1]/asset.png",                // v4-compatible
        "https://[::7f00:1]/asset.png",                 // v4-compatible loopback
        "https://[fd12::1]/asset.png",                  // ULA fc00::/7
        "https://[ff02::1]/asset.png",                  // multicast
        "https://127.0.0.1:18080@evil.com/asset.png",   // userinfo trap (host evil)
        "https://example.com%2f@127.0.0.1/asset.png",   // userinfo + guarded host
        "https://[2606:4700::1111]/asset.png",          // PUBLIC v6 must PASS
        "https://[::ffff:8.8.8.8]/asset.png",           // mapped PUBLIC must PASS
        "https://2130706433.example.com/asset.png",     // numeric-prefix DOMAIN (DNS scope)
    ];

    println!(
        "{:<46} | {:<9} | {:<36} | {}",
        "URL", "guard", "url-crate host", "verdict"
    );
    println!("{}", "-".repeat(120));
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
                    if guarded_v4(octets) {
                        format!("Ipv4 {addr} (GUARDED)")
                    } else {
                        format!("Ipv4 {addr}")
                    }
                }
                Some(url::Host::Ipv6(addr)) => {
                    let label = guarded_v6_label(addr.octets());
                    match label {
                        Some(label) => format!("Ipv6 {addr} ({label})"),
                        None => format!("Ipv6 {addr}"),
                    }
                }
                Some(url::Host::Domain(domain)) => format!("Domain({domain})"),
                None => "<none>".to_string(),
            },
            Err(err) => format!("parse-error {err}"),
        };
        let bypass = guard_verdict == "PASS" && url_side.contains("GUARDED");
        if bypass {
            bypasses += 1;
        }
        println!(
            "{:<46} | {:<9} | {:<36} | {}",
            candidate,
            guard_verdict,
            url_side,
            if bypass { "!! BYPASS" } else { "ok" }
        );
    }
    println!("\nbypass rows: {bypasses}");
}
