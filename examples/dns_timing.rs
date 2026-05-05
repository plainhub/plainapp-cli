// Timing test for .local hostname resolution through reqwest.
// Run with: cargo run --example dns_timing
//
// Tests three scenarios:
//   1. Direct IP              — baseline (should be ~0ms)
//   2. .local no fix          — reqwest's own async DNS (slow on some configs)
//   3. .local + mDNS resolve  — pure mDNS UDP resolver (should be <50ms)

fn main() {
    let local_host = {
        let out = std::process::Command::new("scutil")
            .args(["--get", "LocalHostName"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let local_name = format!("{local_host}.local");
    let server_local = format!("http://{local_name}:9999/");
    let server_ip = "http://127.0.0.1:9999/";

    println!("Mac hostname: {local_name}");
    println!();

    // ── 1. IP direct ─────────────────────────────────────────────────────────
    let t = std::time::Instant::now();
    let c = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let r = c.get(server_ip).send();
    println!("1) IP direct:                {:>6.0}ms  {:?}", ms(t), r.map(|r| r.status().as_u16()));

    // ── 2. .local, no pre-resolve ─────────────────────────────────────────────
    let t = std::time::Instant::now();
    let c = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let r = c.get(&server_local).send();
    println!("2) .local no fix:            {:>6.0}ms  {:?}", ms(t), r.map(|r| r.status().as_u16()));

    // ── 3. .local with pure mDNS resolver ────────────────────────────────────
    let t = std::time::Instant::now();
    let mut builder = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(5));

    let t_dns = std::time::Instant::now();
    if let Some(ipv4) = mdns_resolve(&local_name, 1500) {
        let addr = std::net::SocketAddr::new(std::net::IpAddr::V4(ipv4), 9999);
        println!("   mDNS resolved in         {:>6.0}ms → {addr}", ms(t_dns));
        builder = builder.resolve(&local_name, addr);
    } else {
        println!("   mDNS resolution FAILED");
    }

    let r = builder.build().unwrap().get(&server_local).send();
    println!("3) .local + mDNS .resolve(): {:>6.0}ms  {:?}", ms(t), r.map(|r| r.status().as_u16()));
}

fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Minimal mDNS A-record resolver — inlined for the example binary.
fn mdns_resolve(hostname: &str, timeout_ms: u64) -> Option<std::net::Ipv4Addr> {
    use std::net::UdpSocket;
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(std::time::Duration::from_millis(timeout_ms))).ok()?;
    let _ = sock.set_multicast_ttl_v4(1);
    let query = build_mdns_query(hostname)?;
    sock.send_to(&query, "224.0.0.251:5353").ok()?;
    let mut buf = [0u8; 4096];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => {
                if let Some(ip) = parse_a_answer(&buf[..n]) {
                    return Some(ip);
                }
            }
            Err(_) => return None,
        }
    }
}

fn build_mdns_query(name: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&[0,0, 0,0, 0,1, 0,0, 0,0, 0,0]); // header
    for label in name.trim_end_matches('.').split('.') {
        buf.push(label.len() as u8);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0);
    buf.extend_from_slice(&[0,1, 0x80,1]); // QTYPE=A, QCLASS=IN|QU
    Some(buf)
}

fn parse_a_answer(data: &[u8]) -> Option<std::net::Ipv4Addr> {
    if data.len() < 12 || data[2] & 0x80 == 0 { return None; }
    let qdcount = u16::from_be_bytes([data[4], data[5]]) as usize;
    let ancount = u16::from_be_bytes([data[6], data[7]]) as usize;
    if ancount == 0 { return None; }
    let mut pos = 12usize;
    for _ in 0..qdcount { pos = skip_name(data, pos)?; pos = pos.checked_add(4)?; }
    for _ in 0..ancount {
        pos = skip_name(data, pos)?;
        if pos + 10 > data.len() { return None; }
        let rtype = u16::from_be_bytes([data[pos], data[pos+1]]);
        let rdlen = u16::from_be_bytes([data[pos+8], data[pos+9]]) as usize;
        pos += 10;
        if rtype == 1 && rdlen == 4 && pos + 4 <= data.len() {
            return Some(std::net::Ipv4Addr::new(data[pos], data[pos+1], data[pos+2], data[pos+3]));
        }
        pos = pos.checked_add(rdlen)?;
    }
    None
}

fn skip_name(data: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        if pos >= data.len() { return None; }
        let len = data[pos];
        if len == 0 { return Some(pos + 1); }
        if len & 0xC0 == 0xC0 { return Some(pos + 2); }
        if len & 0xC0 != 0 { return None; }
        pos += 1 + len as usize;
    }
}
