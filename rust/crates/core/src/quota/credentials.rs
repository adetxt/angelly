use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::SystemTime;

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::model::{Provider, QuotaError};
use crate::dates::to_unix_secs;

const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const CODEX_KEYCHAIN_SERVICE: &str = "Codex Auth";

/// A bearer token and, for Codex, the ChatGPT account it belongs to.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_token: String,
    pub account_id: Option<String>,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("access_token", &"<redacted>").field("account_id", &self.account_id).finish()
    }
}

fn jwt_claims(token: &str) -> Option<serde_json::Map<String, Value>> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut payload: String = parts[1].chars().map(|c| match c {
        '-' => '+',
        '_' => '/',
        c => c,
    }).collect();
    payload.push_str(&"=".repeat((4 - payload.len() % 4) % 4));
    let data = base64::engine::general_purpose::STANDARD.decode(payload).ok()?;
    match serde_json::from_slice(&data).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

impl Credentials {
    pub fn decode(data: &[u8], provider: Provider, now: SystemTime) -> Result<Self, QuotaError> {
        let root: Value = serde_json::from_slice(data).map_err(|_| QuotaError::InvalidCredentials)?;
        let root = root.as_object().ok_or(QuotaError::InvalidCredentials)?;
        let now = to_unix_secs(now);
        let (token, account_id): (Option<&str>, Option<String>) = match provider {
            Provider::Codex => {
                if root.get("auth_mode").and_then(Value::as_str) == Some("apikey") {
                    return Err(QuotaError::ApiKeyOnly);
                }
                let tokens = root.get("tokens").and_then(Value::as_object);
                let field = |name: &str| tokens.and_then(|t| t.get(name)).and_then(Value::as_str);
                let token = field("access_token");
                if token.is_none() && root.get("OPENAI_API_KEY").is_some_and(Value::is_string) {
                    return Err(QuotaError::ApiKeyOnly);
                }
                let claims = token.and_then(jwt_claims);
                if let Some(expiry) = claims.as_ref().and_then(|c| c.get("exp")).and_then(Value::as_f64) {
                    if expiry <= now {
                        return Err(QuotaError::Expired);
                    }
                }
                let identity = field("id_token").and_then(jwt_claims).or(claims);
                let auth = identity
                    .as_ref()
                    .and_then(|i| i.get("https://api.openai.com/auth"))
                    .and_then(Value::as_object);
                let account = field("account_id").map(str::to_string).or_else(|| {
                    auth.and_then(|a| a.get("chatgpt_account_id")).and_then(Value::as_str).map(str::to_string)
                });
                (token, account)
            }
            Provider::Claude => {
                let Some(oauth) = root.get("claudeAiOauth").and_then(Value::as_object) else {
                    return match root.get("apiKey").and_then(Value::as_str) {
                        Some(key) if !key.is_empty() => Err(QuotaError::ApiKeyOnly),
                        _ => Err(QuotaError::InvalidCredentials),
                    };
                };
                if let Some(expiry) = oauth.get("expiresAt").and_then(Value::as_f64) {
                    if expiry / 1000.0 <= now {
                        return Err(QuotaError::Expired);
                    }
                }
                (oauth.get("accessToken").and_then(Value::as_str), None)
            }
        };
        let token = token.filter(|t| !t.is_empty() && !t.chars().any(char::is_whitespace));
        match token {
            Some(token) if !account_id.as_deref().is_some_and(|a| a.chars().any(char::is_whitespace)) => {
                Ok(Self { access_token: token.to_string(), account_id })
            }
            _ => Err(QuotaError::InvalidCredentials),
        }
    }
}

/// Where stored logins are read from.
pub trait CredentialSource: Send + Sync {
    /// The file's contents, or `None` when it doesn't exist.
    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, QuotaError>;

    /// A generic-password Keychain item, or `None` when it doesn't exist. When the item
    /// exists but reading it would need user interaction and `allow_interaction` is
    /// false, fails with [`QuotaError::PermissionRequired`].
    fn read_keychain(&self, service: &str, account: Option<&str>, allow_interaction: bool) -> Result<Option<Vec<u8>>, QuotaError>;
}

/// Reads files from disk and, on macOS, the Keychain through `/usr/bin/security`.
///
/// Claude Code writes its item with that tool, so the tool is already trusted by the
/// item's access list and reading through it never prompts. Other items are probed for
/// existence first, which only reads metadata, so a read that would prompt is reported
/// instead of shown unless the caller allows interaction.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemCredentialSource;

const SECURITY_TOOL: &str = "/usr/bin/security";
const ERR_SEC_ITEM_NOT_FOUND: i32 = 44;

fn security(arguments: &[&str]) -> Result<Option<Vec<u8>>, QuotaError> {
    let output = Command::new(SECURITY_TOOL)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| QuotaError::InvalidCredentials)?;
    match output.status.code() {
        Some(0) => Ok(Some(output.stdout)),
        Some(ERR_SEC_ITEM_NOT_FOUND) => Ok(None),
        _ => Err(QuotaError::PermissionRequired),
    }
}

impl CredentialSource for SystemCredentialSource {
    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, QuotaError> {
        match std::fs::read(path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(QuotaError::InvalidCredentials),
        }
    }

    fn read_keychain(&self, service: &str, account: Option<&str>, allow_interaction: bool) -> Result<Option<Vec<u8>>, QuotaError> {
        if !cfg!(target_os = "macos") {
            return Ok(None);
        }
        let mut arguments = vec!["find-generic-password", "-s", service];
        if let Some(account) = account {
            arguments.extend(["-a", account]);
        }
        if service != CLAUDE_KEYCHAIN_SERVICE {
            if security(&arguments)?.is_none() {
                return Ok(None);
            }
            if !allow_interaction {
                return Err(QuotaError::PermissionRequired);
            }
        }
        arguments.push("-w");
        security(&arguments)
    }
}

/// Finds a provider's stored login the way its own CLI does.
pub struct CredentialReader {
    pub environment: HashMap<String, String>,
    pub home: PathBuf,
    pub user: String,
    pub source: Arc<dyn CredentialSource>,
}

/// Collapses `.` and `..` components without touching the file system.
fn standardize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    result.push("..");
                }
            }
            other => result.push(other),
        }
    }
    result
}

impl CredentialReader {
    pub fn from_process() -> Self {
        let environment: HashMap<String, String> = std::env::vars().collect();
        let home = environment
            .get("HOME")
            .or_else(|| environment.get("USERPROFILE").filter(|_| cfg!(windows)))
            .map(PathBuf::from)
            .unwrap_or_default();
        let user = environment
            .get("USER")
            .or_else(|| environment.get("LOGNAME"))
            .or_else(|| environment.get("USERNAME").filter(|_| cfg!(windows)))
            .cloned()
            .unwrap_or_default();
        Self { environment, home, user, source: Arc::new(SystemCredentialSource) }
    }

    pub fn load(&self, provider: Provider, allow_interaction: bool) -> Result<Credentials, QuotaError> {
        let data = match provider {
            Provider::Codex => {
                let root = self.directory("CODEX_HOME", self.home.join(".codex"));
                let mode = self.codex_storage(&root)?;
                if mode == "file" {
                    self.source.read_file(&root.join("auth.json"))?
                } else {
                    let canonical = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
                    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
                    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
                    let account = format!("cli|{}", &hex[..16]);
                    let keychain = self.source.read_keychain(CODEX_KEYCHAIN_SERVICE, Some(&account), allow_interaction)?;
                    match keychain {
                        None if mode == "auto" => self.source.read_file(&root.join("auth.json"))?,
                        other => other,
                    }
                }
            }
            Provider::Claude => {
                let default_root = self.home.join(".claude");
                let config_root = self.directory("CLAUDE_CONFIG_DIR", default_root.clone());
                let root = self.directory("CLAUDE_SECURESTORAGE_CONFIG_DIR", config_root);
                // Claude's shared Keychain service does not identify a custom config directory.
                let native = if root == standardize(&default_root) {
                    self.source.read_keychain(CLAUDE_KEYCHAIN_SERVICE, Some(&self.user), allow_interaction)?
                } else {
                    None
                };
                match native {
                    Some(native) => Some(native),
                    None => self.source.read_file(&root.join(".credentials.json"))?,
                }
            }
        };
        let data = data.ok_or(QuotaError::NotSignedIn)?;
        Credentials::decode(&data, provider, SystemTime::now())
    }

    fn directory(&self, variable: &str, fallback: PathBuf) -> PathBuf {
        match self.environment.get(variable).filter(|p| !p.is_empty()) {
            Some(path) => standardize(Path::new(path)),
            None => standardize(&fallback),
        }
    }

    /// The `cli_auth_credentials_store` setting of Codex's config: `file`, `keyring` or `auto`.
    fn codex_storage(&self, root: &Path) -> Result<&'static str, QuotaError> {
        let Some(data) = self.source.read_file(&root.join("config.toml"))? else { return Ok("file") };
        let Ok(text) = String::from_utf8(data) else { return Ok("file") };
        for line in text.lines() {
            let content = line.trim_matches([' ', '\t']);
            if content.starts_with('[') {
                break;
            }
            let Some((key, value)) = content.split_once('=') else { continue };
            if key.is_empty() || value.is_empty() || key.trim_matches([' ', '\t']) != "cli_auth_credentials_store" {
                continue;
            }
            let value = value.split('#').next().unwrap_or("");
            let mode = value.trim_matches([' ', '\t']).trim_matches(['"', '\'']);
            return match mode {
                "file" => Ok("file"),
                "keyring" => Ok("keyring"),
                "auto" => Ok("auto"),
                _ => Err(QuotaError::InvalidCredentials),
            };
        }
        Ok("file")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{Duration, UNIX_EPOCH};

    fn decode(json: &str, provider: Provider) -> Result<Credentials, QuotaError> {
        Credentials::decode(json.as_bytes(), provider, SystemTime::now())
    }

    #[test]
    fn decodes_tokens_and_rejects_api_keys_and_expiry() {
        let codex = decode(r#"{"tokens":{"access_token":"codex-token","account_id":"account-123"}}"#, Provider::Codex).unwrap();
        assert_eq!(codex.access_token, "codex-token");
        assert_eq!(codex.account_id.as_deref(), Some("account-123"));
        assert_eq!(decode(r#"{"OPENAI_API_KEY":"key"}"#, Provider::Codex), Err(QuotaError::ApiKeyOnly));
        assert_eq!(decode(r#"{"auth_mode":"apikey","tokens":{"access_token":"t"}}"#, Provider::Codex), Err(QuotaError::ApiKeyOnly));
        assert_eq!(
            decode(r#"{"claudeAiOauth":{"accessToken":"claude-token","expiresAt":1000}}"#, Provider::Claude),
            Err(QuotaError::Expired)
        );
        assert_eq!(decode(r#"{"tokens":{}}"#, Provider::Codex), Err(QuotaError::InvalidCredentials));
    }

    #[test]
    fn malformed_claude_credentials_do_not_claim_api_key_authentication() {
        for json in ["{}", r#"{"claudeAiOauth":"broken"}"#, "[]", "nope"] {
            assert_eq!(decode(json, Provider::Claude), Err(QuotaError::InvalidCredentials), "{json}");
        }
        assert_eq!(decode(r#"{"apiKey":"sk"}"#, Provider::Claude), Err(QuotaError::ApiKeyOnly));
    }

    fn jwt(claims: &str) -> String {
        let encode = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        format!("{}.{}.sig", encode("{}"), encode(claims))
    }

    #[test]
    fn codex_jwt_expiry_and_account_claims() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let expired = jwt(r#"{"exp":1700000000}"#);
        let json = format!(r#"{{"tokens":{{"access_token":"{expired}"}}}}"#);
        assert_eq!(Credentials::decode(json.as_bytes(), Provider::Codex, now), Err(QuotaError::Expired));

        let live = jwt(r#"{"exp":1900000000,"https://api.openai.com/auth":{"chatgpt_account_id":"acct-jwt"}}"#);
        let json = format!(r#"{{"tokens":{{"access_token":"{live}"}}}}"#);
        let decoded = Credentials::decode(json.as_bytes(), Provider::Codex, now).unwrap();
        assert_eq!(decoded.account_id.as_deref(), Some("acct-jwt"));

        let id_token = jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct-id"}}"#);
        let json = format!(r#"{{"tokens":{{"access_token":"{live}","id_token":"{id_token}"}}}}"#);
        let decoded = Credentials::decode(json.as_bytes(), Provider::Codex, now).unwrap();
        assert_eq!(decoded.account_id.as_deref(), Some("acct-id"));
    }

    #[test]
    fn whitespace_in_tokens_or_accounts_is_invalid() {
        assert_eq!(decode(r#"{"tokens":{"access_token":"a b"}}"#, Provider::Codex), Err(QuotaError::InvalidCredentials));
        assert_eq!(
            decode(r#"{"tokens":{"access_token":"ok","account_id":"x\ny"}}"#, Provider::Codex),
            Err(QuotaError::InvalidCredentials)
        );
        assert_eq!(decode(r#"{"tokens":{"access_token":""}}"#, Provider::Codex), Err(QuotaError::InvalidCredentials));
    }

    #[test]
    fn debug_output_redacts_the_token() {
        let credentials = Credentials { access_token: "secret-fixture".into(), account_id: None };
        assert!(!format!("{credentials:?}").contains("secret-fixture"));
    }

    type FileFn = Box<dyn Fn(&Path) -> Result<Option<Vec<u8>>, QuotaError> + Send + Sync>;
    type KeychainFn = Box<dyn Fn(&str, Option<&str>, bool) -> Result<Option<Vec<u8>>, QuotaError> + Send + Sync>;

    struct Mock {
        file: FileFn,
        keychain: KeychainFn,
    }

    impl CredentialSource for Mock {
        fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, QuotaError> {
            (self.file)(path)
        }
        fn read_keychain(&self, service: &str, account: Option<&str>, interaction: bool) -> Result<Option<Vec<u8>>, QuotaError> {
            (self.keychain)(service, account, interaction)
        }
    }

    fn reader(environment: &[(&str, &str)], file: FileFn, keychain: KeychainFn) -> CredentialReader {
        CredentialReader {
            environment: environment.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            home: PathBuf::from("/test-home"),
            user: "tester".into(),
            source: Arc::new(Mock { file, keychain }),
        }
    }

    fn no_keychain() -> KeychainFn {
        Box::new(|_, _, _| panic!("keychain must not be queried"))
    }

    #[test]
    fn configured_claude_directory_never_reads_default_keychain() {
        let reader = reader(
            &[("CLAUDE_CONFIG_DIR", "/profiles/work")],
            Box::new(|path| {
                assert_eq!(path, Path::new("/profiles/work/.credentials.json"));
                Ok(None)
            }),
            no_keychain(),
        );
        assert_eq!(reader.load(Provider::Claude, false), Err(QuotaError::NotSignedIn));
    }

    #[test]
    fn secure_storage_directory_overrides_config_directory() {
        let reader = reader(
            &[("CLAUDE_CONFIG_DIR", "/a"), ("CLAUDE_SECURESTORAGE_CONFIG_DIR", "/b/../c")],
            Box::new(|path| {
                assert_eq!(path, Path::new("/c/.credentials.json"));
                Ok(None)
            }),
            no_keychain(),
        );
        assert_eq!(reader.load(Provider::Claude, false), Err(QuotaError::NotSignedIn));
    }

    #[test]
    fn malformed_file_does_not_fall_through_to_another_credential_store() {
        let reader = reader(
            &[],
            Box::new(|path| Ok((path.file_name().unwrap() == "auth.json").then(|| b"broken".to_vec()))),
            no_keychain(),
        );
        assert_eq!(reader.load(Provider::Codex, false), Err(QuotaError::InvalidCredentials));
    }

    #[test]
    fn codex_uses_configured_home_and_claude_decodes_native_keychain_payload() {
        let codex = reader(
            &[("CODEX_HOME", "/profiles/codex")],
            Box::new(|path| {
                if path.file_name().unwrap() == "config.toml" {
                    return Ok(None);
                }
                assert_eq!(path, Path::new("/profiles/codex/auth.json"));
                Ok(Some(br#"{"tokens":{"access_token":"local-token"}}"#.to_vec()))
            }),
            no_keychain(),
        );
        assert_eq!(codex.load(Provider::Codex, false).unwrap().access_token, "local-token");

        let claude = reader(
            &[],
            Box::new(|_| Ok(None)),
            Box::new(|service, account, interaction| {
                assert_eq!(service, "Claude Code-credentials");
                assert_eq!(account, Some("tester"));
                assert!(!interaction);
                Ok(Some(br#"{"claudeAiOauth":{"accessToken":"claude-token","scopes":["user:profile"]}}"#.to_vec()))
            }),
        );
        assert_eq!(claude.load(Provider::Claude, false).unwrap().access_token, "claude-token");
    }

    #[test]
    fn claude_default_profile_falls_back_to_file_when_keychain_has_no_item() {
        let reader = reader(
            &[],
            Box::new(|path| {
                assert_eq!(path, Path::new("/test-home/.claude/.credentials.json"));
                Ok(Some(br#"{"claudeAiOauth":{"accessToken":"file-token"}}"#.to_vec()))
            }),
            Box::new(|_, _, _| Ok(None)),
        );
        assert_eq!(reader.load(Provider::Claude, false).unwrap().access_token, "file-token");
    }

    fn codex_config(storage: &'static str) -> FileFn {
        Box::new(move |path| match path.file_name().unwrap().to_str().unwrap() {
            "config.toml" => Ok(Some(format!("model = \"x\"\ncli_auth_credentials_store = \"{storage}\" # note\n[tui]\ncli_auth_credentials_store = \"file\"\n").into_bytes())),
            "auth.json" => Ok(Some(br#"{"tokens":{"access_token":"file-token"}}"#.to_vec())),
            other => panic!("unexpected read {other}"),
        })
    }

    #[test]
    fn codex_keyring_storage_reads_the_hashed_keychain_account() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = seen.clone();
        let reader = reader(
            &[("CODEX_HOME", "/profiles/codex")],
            codex_config("keyring"),
            Box::new(move |service, account, interaction| {
                recorded.lock().unwrap().push((service.to_string(), account.map(str::to_string), interaction));
                Ok(Some(br#"{"tokens":{"access_token":"keychain-token"}}"#.to_vec()))
            }),
        );
        assert_eq!(reader.load(Provider::Codex, true).unwrap().access_token, "keychain-token");
        let seen = seen.lock().unwrap();
        let expected = Sha256::digest(b"/profiles/codex");
        let hex: String = expected.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(seen[0], ("Codex Auth".to_string(), Some(format!("cli|{}", &hex[..16])), true));
    }

    #[test]
    fn codex_auto_storage_falls_back_to_file_only_when_keychain_is_empty() {
        let empty = reader(&[("CODEX_HOME", "/profiles/codex")], codex_config("auto"), Box::new(|_, _, _| Ok(None)));
        assert_eq!(empty.load(Provider::Codex, false).unwrap().access_token, "file-token");

        let denied = reader(
            &[("CODEX_HOME", "/profiles/codex")],
            codex_config("auto"),
            Box::new(|_, _, _| Err(QuotaError::PermissionRequired)),
        );
        assert_eq!(denied.load(Provider::Codex, false), Err(QuotaError::PermissionRequired));

        let keyring = reader(&[("CODEX_HOME", "/profiles/codex")], codex_config("keyring"), Box::new(|_, _, _| Ok(None)));
        assert_eq!(keyring.load(Provider::Codex, false), Err(QuotaError::NotSignedIn));
    }

    #[test]
    fn unknown_codex_storage_mode_is_invalid() {
        let reader = reader(&[], codex_config("cloud"), no_keychain());
        assert_eq!(reader.load(Provider::Codex, false), Err(QuotaError::InvalidCredentials));
    }

    #[test]
    fn credential_files_are_read_from_a_scoped_temporary_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("auth.json"), r#"{"tokens":{"access_token":"file-fixture"}}"#).unwrap();
        let reader = CredentialReader {
            environment: HashMap::from([("CODEX_HOME".to_string(), dir.path().to_string_lossy().into_owned())]),
            home: PathBuf::from("/nonexistent-home"),
            user: String::new(),
            source: Arc::new(SystemCredentialSource),
        };
        assert_eq!(reader.load(Provider::Codex, false).unwrap().access_token, "file-fixture");
    }

    #[test]
    fn standardize_collapses_dot_segments() {
        assert_eq!(standardize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
    }
}
