/// Pure-Rust minimal mDNS resolver for `.local` hostnames.
///
/// Strategy: create TWO UDP sockets and poll both:
///
///   1. **Send socket** — ephemeral port; sends the query and catches unicast
///      (QU) responses.  Works for Apple devices and this Mac's own name.
///
///   2. **Multicast receiver** — bound to 0.0.0.0:5353 with SO_REUSEADDR +
///      SO_REUSEPORT, joined to 224.0.0.251.  Catches multicast responses,
///      which is what Android's NSD stack sends (it ignores the QU bit and
///      always replies on the multicast group).
///
/// Without socket 2, Android phones never reply to our query because they
/// respond on `224.0.0.251:5353` and we were only listening on the ephemeral
/// port.  The system resolver (`getaddrinfo`) works eventually because
/// `mDNSResponder` already listens on port 5353, but it has a 2-3 s unicast-
/// DNS-first delay before it falls back to mDNS.
use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

const MDNS_ADDR: &str = "224.0.0.251:5353";

/// Resolve `hostname` (e.g. `"cn.local"`) to an IPv4 address using mDNS.
/// Returns `None` on timeout or error.
pub fn resolve_ipv4(hostname: &str, timeout: Duration) -> Option<Ipv4Addr> {
    // Create multicast receiver BEFORE sending, so we don't miss fast replies.
    let mc_sock = create_multicast_receiver();

    let send_sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    let poll = Duration::from_millis(50);
    send_sock.set_read_timeout(Some(poll)).ok()?;
    if let Some(ref s) = mc_sock {
        s.set_read_timeout(Some(poll)).ok()?;
    }
    let _ = send_sock.set_multicast_ttl_v4(1); // link-local only

    let query = build_query(hostname)?;
    send_sock.send_to(&query, MDNS_ADDR).ok()?;

    let deadline = std::time::Instant::now() + timeout;
    let mut buf = [0u8; 4096];

    while std::time::Instant::now() < deadline {
        // Check ephemeral socket (unicast / QU responses)
        if let Ok((n, _)) = send_sock.recv_from(&mut buf) {
            if let Some(ip) = parse_a_answer(&buf[..n], hostname) {
                return Some(ip);
            }
        }
        // Check multicast socket (multicast responses, e.g. from Android)
        if let Some(ref mc) = mc_sock {
            if let Ok((n, _)) = mc.recv_from(&mut buf) {
                if let Some(ip) = parse_a_answer(&buf[..n], hostname) {
                    return Some(ip);
                }
            }
        }
    }
    None
}

// ── Multicast receiver socket ─────────────────────────────────────────────────

/// Create a UDP socket bound to 0.0.0.0:5353 joined to the mDNS multicast
/// group, so we can receive multicast DNS responses.  Uses raw syscalls to
/// set SO_REUSEADDR + SO_REUSEPORT before binding (required to share port 5353
/// with the system mDNS daemon).  Returns `None` on any failure.
#[cfg(unix)]
fn create_multicast_receiver() -> Option<UdpSocket> {
    use std::ffi::c_void;
    use std::os::unix::io::FromRawFd;

    extern "C" {
        fn socket(domain: i32, sock_type: i32, protocol: i32) -> i32;
        fn setsockopt(fd: i32, level: i32, name: i32, val: *const c_void, len: u32) -> i32;
        fn bind(fd: i32, addr: *const c_void, len: u32) -> i32;
        fn close(fd: i32) -> i32;
    }

    // ── platform constants ────────────────────────────────────────────────────
    const AF_INET: i32 = 2;
    const SOCK_DGRAM: i32 = 2;
    const IPPROTO_IP: i32 = 0;

    #[cfg(target_os = "macos")] const SOL_SOCKET: i32 = 0xffff;
    #[cfg(not(target_os = "macos"))] const SOL_SOCKET: i32 = 1;

    #[cfg(target_os = "macos")] const SO_REUSEADDR: i32 = 4;
    #[cfg(not(target_os = "macos"))] const SO_REUSEADDR: i32 = 2;

    #[cfg(target_os = "macos")] const SO_REUSEPORT: i32 = 0x200;
    #[cfg(not(target_os = "macos"))] const SO_REUSEPORT: i32 = 15;

    #[cfg(target_os = "macos")] const IP_ADD_MEMBERSHIP: i32 = 12;
    #[cfg(not(target_os = "macos"))] const IP_ADD_MEMBERSHIP: i32 = 35;

    unsafe {
        let fd = socket(AF_INET, SOCK_DGRAM, 17 /* IPPROTO_UDP */);
        if fd < 0 {
            return None;
        }
        let one: i32 = 1;
        let p = &one as *const i32 as *const c_void;
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, p, 4);
        setsockopt(fd, SOL_SOCKET, SO_REUSEPORT, p, 4);

        // sockaddr_in for 0.0.0.0:5353
        // macOS adds a sin_len byte before sin_family.
        let port = 5353u16.to_be_bytes();
        #[cfg(target_os = "macos")]
        let sa: [u8; 16] = [16, 2, port[0], port[1], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        #[cfg(not(target_os = "macos"))]
        let sa: [u8; 16] = [2, 0, port[0], port[1], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

        if bind(fd, sa.as_ptr() as *const c_void, 16) < 0 {
            close(fd);
            return None;
        }

        // struct ip_mreq { imr_multiaddr(4 bytes), imr_interface(4 bytes) }
        // 224.0.0.251 is already in network byte order as written.
        let mreq: [u8; 8] = [224, 0, 0, 251, 0, 0, 0, 0];
        // Ignore the return value: if the join fails the socket still works,
        // we just won't receive multicast (unicast fallback still applies).
        setsockopt(fd, IPPROTO_IP, IP_ADD_MEMBERSHIP, mreq.as_ptr() as *const c_void, 8);

        Some(UdpSocket::from_raw_fd(fd))
    }
}

#[cfg(not(unix))]
fn create_multicast_receiver() -> Option<UdpSocket> {
    None // Windows support not implemented; unicast path still works
}

// ── DNS packet builder ────────────────────────────────────────────────────────

/// Build a minimal DNS query for an A record.
/// Sets the QU bit (0x8000) in QCLASS to request a unicast response so the
/// reply comes back to our ephemeral source port instead of the multicast group.
fn build_query(name: &str) -> Option<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::with_capacity(64);
    // Header (12 bytes)
    buf.extend_from_slice(&[0, 0]); // Transaction ID = 0 (required by mDNS)
    buf.extend_from_slice(&[0, 0]); // Flags = standard query
    buf.extend_from_slice(&[0, 1]); // QDCOUNT = 1
    buf.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // AN/NS/AR = 0
    // QNAME — encode each label (RFC 1035 §3.1)
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        buf.push(label.len() as u8);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0); // root label
    buf.extend_from_slice(&[0, 1]);    // QTYPE  = A
    buf.extend_from_slice(&[0x80, 1]); // QCLASS = IN | QU bit
    Some(buf)
}

// ── DNS response parser ───────────────────────────────────────────────────────

/// Return the IPv4 address from the first A record in `data` whose owner name
/// matches `want_name` (case-insensitive, trailing dot optional).
fn parse_a_answer(data: &[u8], want_name: &str) -> Option<Ipv4Addr> {
    if data.len() < 12 || data[2] & 0x80 == 0 {
        return None; // too short or not a response
    }
    let qdcount = u16::from_be_bytes([data[4], data[5]]) as usize;
    let ancount = u16::from_be_bytes([data[6], data[7]]) as usize;
    if ancount == 0 {
        return None;
    }
    let mut pos = 12usize;
    for _ in 0..qdcount {
        pos = skip_name(data, pos)?;
        pos = pos.checked_add(4)?;
    }
    for _ in 0..ancount {
        let name_start = pos;
        pos = skip_name(data, pos)?;
        if pos + 10 > data.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let rdlen = u16::from_be_bytes([data[pos + 8], data[pos + 9]]) as usize;
        pos += 10;
        if rtype == 1 /* A */ && rdlen == 4 && pos + 4 <= data.len() {
            if name_matches(data, name_start, want_name) {
                return Some(Ipv4Addr::new(
                    data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
                ));
            }
        }
        pos = pos.checked_add(rdlen)?;
    }
    None
}

/// Decode the DNS name at `pos` in `data` and compare it to `want`
/// (case-insensitive; trailing dots ignored).
fn name_matches(data: &[u8], mut pos: usize, want: &str) -> bool {
    let want = want.trim_end_matches('.');
    let mut decoded = String::with_capacity(want.len() + 1);
    let mut hops = 0u8;
    loop {
        if hops > 20 || pos >= data.len() {
            return false;
        }
        let len = data[pos];
        if len == 0 {
            break;
        }
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= data.len() {
                return false;
            }
            pos = (((len & 0x3F) as usize) << 8) | data[pos + 1] as usize;
            hops += 1;
            continue;
        }
        if len & 0xC0 != 0 {
            return false;
        }
        let end = pos + 1 + len as usize;
        if end > data.len() {
            return false;
        }
        if !decoded.is_empty() {
            decoded.push('.');
        }
        decoded.push_str(std::str::from_utf8(&data[pos + 1..end]).unwrap_or(""));
        pos = end;
    }
    decoded.eq_ignore_ascii_case(want)
}

/// Skip a DNS name (handles both plain labels and pointer compression).
fn skip_name(data: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        if pos >= data.len() {
            return None;
        }
        let len = data[pos];
        if len == 0 {
            return Some(pos + 1);
        }
        if len & 0xC0 == 0xC0 {
            return Some(pos + 2);
        }
        if len & 0xC0 != 0 {
            return None;
        }
        pos += 1 + len as usize;
    }
}

// ── dns-sd subprocess resolver ────────────────────────────────────────────────

/// Resolve a `.local` hostname by spawning `dns-sd -G v4 <hostname>`.
///
/// `dns-sd` uses the native `DNSServiceGetAddrInfo` API, which goes directly
/// to `mDNSResponder` and returns results from its multicast-DNS cache without
/// the unicast-DNS-first delay that affects `getaddrinfo` / `to_socket_addrs`
/// when a VPN adds a catch-all resolver to the system configuration.
///
/// Typical latency: < 100 ms.  Falls back gracefully on non-macOS or if the
/// `dns-sd` binary is unavailable.
pub fn resolve_via_dns_sd(hostname: &str, timeout: Duration) -> Option<Ipv4Addr> {
    use std::io::{BufRead, BufReader};
    use std::net::Ipv4Addr;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;

    let mut child = Command::new("dns-sd")
        .args(["-G", "v4", hostname])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let stdout = child.stdout.take()?;
    let needle = hostname.trim_end_matches('.').to_ascii_lowercase();

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let lower = line.to_ascii_lowercase();
            if lower.contains(&needle) {
                for field in line.split_whitespace() {
                    if let Ok(ip) = field.parse::<Ipv4Addr>() {
                        let _ = tx.send(ip);
                        return;
                    }
                }
            }
        }
    });

    let result = rx.recv_timeout(timeout).ok();
    let _ = child.kill();
    let _ = child.wait();
    result
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_packet_structure() {
        let q = build_query("cn.local").unwrap();
        assert_eq!(&q[0..2], &[0, 0], "ID must be 0 for mDNS");
        assert_eq!(&q[2..4], &[0, 0], "flags = standard query");
        assert_eq!(&q[4..6], &[0, 1], "QDCOUNT = 1");
        assert_eq!(q[12], 2);
        assert_eq!(&q[13..15], b"cn");
        assert_eq!(q[15], 5);
        assert_eq!(&q[16..21], b"local");
        assert_eq!(q[21], 0);
        assert_eq!(&q[22..24], &[0, 1]);
        assert_eq!(&q[24..26], &[0x80, 1]);
    }

    #[test]
    fn parse_minimal_a_response() {
        // ANCOUNT=1, owner name = root label [0], A record = 192.168.1.42
        let pkt: Vec<u8> = vec![
            0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0,
            // answer: name root (empty), A, IN, ttl, rdlen=4, 192.168.1.42
            0,          // root label → decodes to ""
            0, 1, 0, 1, 0, 0, 0, 120, 0, 4,
            192, 168, 1, 42,
        ];
        // empty name "" matches empty want ""
        let ip = parse_a_answer(&pkt, "").unwrap();
        assert_eq!(ip, Ipv4Addr::new(192, 168, 1, 42));
    }

    #[test]
    fn name_matching() {
        // Build a tiny packet: header(12) + name "cn.local" + A record
        let mut pkt: Vec<u8> = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0];
        pkt.extend_from_slice(&[2, b'c', b'n', 5, b'l', b'o', b'c', b'a', b'l', 0]);
        pkt.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 10, 0, 0, 1]);
        assert!(parse_a_answer(&pkt, "cn.local").is_some());
        assert!(parse_a_answer(&pkt, "other.local").is_none());
        assert!(parse_a_answer(&pkt, "CN.LOCAL").is_some()); // case-insensitive
    }

    /// Live test — query this Mac's own .local hostname.
    /// Run with: cargo test mdns_live -- --nocapture --ignored
    #[test]
    #[ignore]
    fn mdns_live_self() {
        let name = format!(
            "{}.local",
            std::process::Command::new("scutil")
                .args(["--get", "LocalHostName"])
                .output()
                .unwrap()
                .stdout
                .iter()
                .map(|&b| b as char)
                .collect::<String>()
                .trim()
                .to_owned()
        );
        println!("Querying mDNS for: {name}");
        let t = std::time::Instant::now();
        let ip = resolve_ipv4(&name, Duration::from_secs(2));
        println!("Result: {ip:?}  ({:.0}ms)", t.elapsed().as_secs_f64() * 1000.0);
        assert!(ip.is_some(), "mDNS resolution failed for {name}");
    }

    /// Live test — query the Android phone via dns-sd subprocess.
    /// Run with: cargo test mdns_dns_sd -- --nocapture --ignored
    #[test]
    #[ignore]
    fn mdns_dns_sd() {
        let t = std::time::Instant::now();
        let ip = resolve_via_dns_sd("cn.local", Duration::from_millis(800));
        println!("cn.local via dns-sd → {ip:?}  ({:.0}ms)", t.elapsed().as_secs_f64() * 1000.0);
        assert!(ip.is_some(), "dns-sd resolution failed for cn.local");
    }

    /// Live test — query the Android phone cn.local.
    /// Run with: cargo test mdns_android -- --nocapture --ignored
    #[test]
    #[ignore]
    fn mdns_android() {
        let t = std::time::Instant::now();
        let ip = resolve_ipv4("cn.local", Duration::from_secs(2));
        println!("cn.local → {ip:?}  ({:.0}ms)", t.elapsed().as_secs_f64() * 1000.0);
        assert!(ip.is_some(), "mDNS resolution failed for cn.local");
    }
}
