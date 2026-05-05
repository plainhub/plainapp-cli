# plainapp-cli

A Rust command-line tool for querying and managing your Android phone via the
[PlainApp](https://github.com/plainhub/plain-app) GraphQL API.

Designed as an **AI-callable CLI** — feed it to any AI agent that can run shell
commands to inspect and control phone data (contacts, SMS, files, notes, etc.).

---

## How it works

PlainApp exposes a GraphQL server on your local network.  Every request is
end-to-end encrypted with **XChaCha20-Poly1305** using a 32-byte session token.

The wire protocol (identical to `plain-web`):

```
plaintext  →  wrap_payload  →  encrypt  →  POST /graphql  →  decrypt  →  JSON
```

**Payload envelope** (anti-replay, matches `ReplayGuard.kt`):

```
<unix_ms> | <16-hex-nonce> | <graphql_json>
```

---

## Installation

### One-liner (macOS & Linux)

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/plainhub/plainapp-cli/main/install.sh)
```

The script auto-detects your OS/arch, downloads the right pre-built binary from
the [latest GitHub Release](https://github.com/plainhub/plainapp-cli/releases), and
installs it to `/usr/local/bin/`.  Override the destination with `INSTALL_DIR`:

```bash
INSTALL_DIR=~/.local/bin bash <(curl -fsSL .../install.sh)
```

### Build from source

Prerequisites: [Rust toolchain](https://rustup.rs/) ≥ 1.75

```bash
cd plainapp-cli
cargo build --release
# binary → target/release/plainapp-cli
```

Optionally install globally:

```bash
cargo install --path .
```

## Setup

### 1 — Create an API token in PlainApp

1. Open **PlainApp** on your Android device.
2. Go to **Settings → Sessions & API Tokens → "API Tokens"** tab.
3. Tap **+**, enter a name (e.g. `my-laptop`), and confirm.
4. Note the **Client ID** and **Token** shown.

### 2 — Write the config file

```bash
plainapp-cli --init
```

This creates `~/.config/plainapp-cli/config.toml` with commented placeholders.
Edit it:

```toml
# Base URL of PlainApp's HTTP server on your phone (HTTP or HTTPS)
api_url   = "https://192.168.1.100:8443"

# Copied from PlainApp → Settings → Sessions & API Tokens
client_id = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
token     = "BASE64_ENCODED_32_BYTE_TOKEN"
```

> **TLS note**: PlainApp uses a self-signed certificate.  The CLI accepts it
> automatically — no extra setup required.

Custom config path:

```bash
plainapp-cli --config /path/to/config.toml --schema
```

---

## Usage

```
plainapp-cli [OPTIONS] <--schema | --exec <JSON> | --init>

Options:
  -c, --config <FILE>   Path to config file [default: ~/.config/plainapp-cli/config.toml]
  -s, --schema          Fetch the full GraphQL schema (introspection) as JSON
  -e, --exec <JSON>     Execute a GraphQL query/mutation
      --init            Write an example config file and exit
  -h, --help            Print help
  -V, --version         Print version
```

### Fetch the schema

```bash
plainapp-cli --schema
# or short form:
plainapp-cli -s
```

Runs a full GraphQL introspection query and prints the schema as pretty-printed
JSON.  Pipe to `jq` for filtering:

```bash
plainapp-cli -s | jq '.data.__schema.types[] | select(.name | startswith("Chat"))'
```

### Execute a query

```bash
plainapp-cli --exec '<json>'
# or short form:
plainapp-cli -e '<json>'
```

The `<json>` argument must be a JSON object with a `query` field (and optionally
`variables`).

**Examples:**

```bash
# List chat channels
plainapp-cli -e '{
  "query": "query GetChatChannels { chatChannels { id name owner version status createdAt updatedAt members { id status } } }",
  "variables": null
}'

# List notes
plainapp-cli -e '{"query":"{ app { battery sdcardPath  } }","variables":null}'

# List SMS conversations
plainapp-cli -e '{"query":"{ smses(limit:20,offset:0) { id address body date } }","variables":null}'

# Delete a file
plainapp-cli -e '{
  "query": "mutation DeleteFiles($paths:[String!]!) { deleteFiles(paths:$paths) }",
  "variables": {"paths":["/sdcard/tmp/test.txt"]}
}'
```

---

## Config file reference

| Key         | Description                                                    |
|-------------|----------------------------------------------------------------|
| `api_url`   | Base URL, e.g. `https://192.168.1.100:8443`                   |
| `client_id` | Client ID from PlainApp settings                               |
| `token`     | Base64-encoded 32-byte ChaCha20 key from PlainApp settings     |

The config file is TOML.  You can maintain multiple files and select them with
`--config`.

---

## Security notes

- **Never commit your config file** — it contains a symmetric encryption key.
- Tokens can be revoked at any time from PlainApp → Settings → Sessions & API Tokens.
- The CLI uses `rustls` with `danger_accept_invalid_certs` enabled because
  PlainApp generates a self-signed certificate.  Only connect to trusted local
  networks.

---

## Project structure

```
cli/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs      # CLI entry point, argument parsing
    ├── config.rs    # Config file loading / example generation
    ├── crypto.rs    # XChaCha20-Poly1305 encrypt / decrypt + payload wrapping
    └── api.rs       # HTTP client, GraphQL introspection query
```
