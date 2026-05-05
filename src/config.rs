use crate::Result;
use std::fs;
use std::path::PathBuf;

pub struct Config {
    pub api_url: String,
    pub client_id: String,
    pub token: String,
}

/// Default path: `$XDG_CONFIG_HOME/plainapp-cli/config.toml`
/// (falls back to `~/.config/plainapp-cli/config.toml`).
pub fn default_config_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    base.join("plainapp-cli").join("config.toml")
}

/// Load and parse the config file.  Accepts `None` to use the default path.
pub fn load(path: Option<PathBuf>) -> Result<Config> {
    let path = path.unwrap_or_else(default_config_path);
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
    parse(&content).map_err(|e| format!("invalid config {}: {e}", path.display()).into())
}

/// Write a commented example config file.  Fails if the file already exists.
pub fn write_example(path: &PathBuf) -> Result<()> {
    if path.exists() {
        return Err(format!(
            "config file already exists: {}\nDelete it first if you want to regenerate.",
            path.display()
        )
        .into());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, EXAMPLE_CONFIG)?;
    Ok(())
}

// ── parser ─────────────────────────────────────────────────────────────────────────────

/// Parse a TOML-subset config file: `key = "value"` lines, `#` comments.
fn parse(content: &str) -> std::result::Result<Config, String> {
    let mut api_url: Option<String> = None;
    let mut client_id: Option<String> = None;
    let mut token: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "api_url"   => api_url   = Some(v),
                "client_id" => client_id = Some(v),
                "token"     => token     = Some(v),
                _           => {}
            }
        }
    }

    Ok(Config {
        api_url:   api_url.ok_or("missing `api_url`")?,
        client_id: client_id.ok_or("missing `client_id`")?,
        token:     token.ok_or("missing `token`")?,
    })
}

const EXAMPLE_CONFIG: &str = r#"# plainapp-cli configuration
# ─────────────────────────────────────────────────────────────────────────────
# How to obtain the values below:
#   1. Open PlainApp on your Android device.
#   2. Go to Settings → Sessions & API Tokens → “API Tokens” tab.
#   3. Tap + to create a new token and give it a name.
#   4. Copy the displayed “Client ID” into `client_id`.
#   5. Copy the displayed “Token” (base64 string) into `token`.
# ─────────────────────────────────────────────────────────────────────────────

# Base URL of the PlainApp HTTP server running on your phone.
api_url   = "https://192.168.1.100:8443"

# Client ID from PlainApp → Settings → Sessions & API Tokens
client_id = "YOUR_CLIENT_ID_HERE"

# Base64-encoded 32-byte ChaCha20 key shown next to the client ID
token     = "YOUR_BASE64_TOKEN_HERE"
"#;
