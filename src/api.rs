/// HTTP client that talks to the PlainApp GraphQL endpoint.
///
/// All requests are encrypted with XChaCha20-Poly1305 using the session token.
/// The server expects:
///   - Header `c-id: <client_id>`
///   - Body: `encrypt(key, "<ts>|<nonce>|<graphql_json>")`
/// The response body is likewise encrypted and must be decrypted with the same key.
use crate::{config::Config, crypto, Result};

/// Standard GraphQL introspection query — returns the full schema as JSON.
const INTROSPECTION_QUERY: &str = concat!(
    r#"{"query":"{ __schema { "#,
    r#"queryType { name } "#,
    r#"mutationType { name } "#,
    r#"subscriptionType { name } "#,
    r#"types { "#,
    r#"name kind description "#,
    r#"fields(includeDeprecated: true) { "#,
    r#"name description isDeprecated deprecationReason "#,
    r#"type { name kind ofType { name kind ofType { name kind ofType { name kind } } } } "#,
    r#"args { name description defaultValue "#,
    r#"type { name kind ofType { name kind ofType { name kind } } } } } "#,
    r#"inputFields { name description defaultValue "#,
    r#"type { name kind ofType { name kind ofType { name kind } } } } "#,
    r#"enumValues(includeDeprecated: true) { name description isDeprecated } "#,
    r#"interfaces { name kind } "#,
    r#"possibleTypes { name kind } "#,
    r#"} } }","variables":null}"#
);

/// Execute any GraphQL JSON payload against the PlainApp server.
///
/// `json_payload` must be a valid JSON object such as
/// `{"query":"{ ... }","variables":null}`.
pub fn execute(config: &Config, json_payload: &str) -> Result<String> {
    let key = crypto::base64_decode(&config.token)?;
    let payload = crypto::wrap_payload(json_payload);
    let encrypted_body = crypto::encrypt(&key, &payload)?;

    let url = format!("{}/graphql", config.api_url.trim_end_matches('/'));
    let client = build_http_client(&config.api_url)?;

    let response = client
        .post(&url)
        .header("c-id", &config.client_id)
        .body(encrypted_body)
        .send()
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = response.status();
    match status.as_u16() {
        401 => return Err("unauthorized -- check client_id and token in config".into()),
        403 => return Err("forbidden -- web access may be disabled on the device".into()),
        s if s >= 400 => return Err(format!("server returned HTTP {s}").into()),
        _ => {}
    }

    let body = response
        .bytes()
        .map_err(|e| format!("failed to read response body: {e}"))?;

    crypto::decrypt(&key, &body)
}

/// Fetch and return the full GraphQL schema via introspection.
pub fn fetch_schema(config: &Config) -> Result<String> {
    execute(config, INTROSPECTION_QUERY)
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build an HTTP client.
///
/// For `.local` (mDNS) hostnames, we query the mDNS multicast group
/// (224.0.0.251:5353) directly to get the IPv4 address, then inject it
/// via `.resolve()` so reqwest's internal async DNS — which does NOT handle
/// `.local` — never runs.  This eliminates the ~5 s timeout that occurs when
/// reqwest's async DNS tries every unicast server before giving up on
/// mDNS-only names.  For regular hostnames we fall back to `to_socket_addrs`.
fn build_http_client(api_url: &str) -> Result<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(std::time::Duration::from_secs(30));

    if let Some((host, addr)) = resolve_host(api_url) {
        builder = builder.resolve(&host, addr);
    }

    builder
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}").into())
}

/// Extract hostname and port from a URL and resolve to a `SocketAddr`.
/// Returns `(hostname, SocketAddr)` on success, `None` if resolution fails or
/// if the URL uses a plain IP address (no resolution needed).
///
/// For `.local` hostnames, uses the built-in mDNS resolver which directly
/// queries 224.0.0.251:5353 — completely bypassing the system resolver and
/// its slow unicast-DNS-first path.
fn resolve_host(url: &str) -> Option<(String, std::net::SocketAddr)> {
    // Strip scheme
    let rest = url.find("://").map(|i| &url[i + 3..]).unwrap_or(url);
    // Strip path
    let host_port = rest.split('/').next()?;

    // Split host and port
    let (host, port_str) = if let Some(idx) = host_port.rfind(':') {
        (&host_port[..idx], &host_port[idx + 1..])
    } else {
        let default_port = if url.starts_with("https") { "443" } else { "80" };
        (host_port, default_port)
    };

    let port: u16 = port_str.parse().ok()?;

    // Skip if already an IP address — no DNS needed
    if host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }

    let ip = if host.ends_with(".local") || host == "local" {
        // Primary: spawn `dns-sd -G v4 <host>` which calls the native
        // DNSServiceGetAddrInfo API.  Bypasses the VPN-induced unicast-DNS-
        // first delay that affects getaddrinfo.  Typical latency: < 100 ms.
        if let Some(ipv4) = crate::mdns::resolve_via_dns_sd(
            host,
            std::time::Duration::from_millis(800),
        ) {
            std::net::IpAddr::V4(ipv4)
        } else if let Some(ipv4) = crate::mdns::resolve_ipv4(
            // Fallback: raw multicast mDNS (works for Apple devices).
            host,
            std::time::Duration::from_millis(1500),
        ) {
            std::net::IpAddr::V4(ipv4)
        } else {
            // Last resort: system resolver (has VPN-induced ~2.5 s delay).
            use std::net::ToSocketAddrs;
            format!("{host}:{port}")
                .to_socket_addrs()
                .ok()?
                .find(|a| a.is_ipv4())
                .or_else(|| format!("{host}:{port}").to_socket_addrs().ok()?.next())
                .map(|a| a.ip())?
        }
    } else {
        // Regular hostname: use the OS resolver.
        use std::net::ToSocketAddrs;
        format!("{host}:{port}")
            .to_socket_addrs()
            .ok()?
            .find(|a| a.is_ipv4())
            .or_else(|| format!("{host}:{port}").to_socket_addrs().ok()?.next())
            .map(|a| a.ip())?
    };

    Some((host.to_string(), std::net::SocketAddr::new(ip, port)))
}
