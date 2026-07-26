//! Cipher tool — encryption, hashing, encoding, and password generation.
//!
//! Pure-Rust implementations where possible; falls back to openssl CLI only
//! for file encryption. All hash/encode/password operations are zero-dependency.
//!
//! Degradation strategy:
//! - Hash/encode/password: always available (pure Rust)
//! - File encryption: tries openssl CLI, falls back to XOR obfuscation with warning

use async_trait::async_trait;
use rand::Rng;
use serde::Deserialize;
use serde_json::json;

use crate::tool::ToolDegradation;
use benshu_infra::error::Error;
use benshu_infra::{Tool, ToolDefinition};

const MAX_CIPHER_FILE_BYTES: u64 = 100 * 1024 * 1024;

pub struct CipherTool;

#[derive(Deserialize)]
struct CipherArgs {
    action: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    output: String,
    #[serde(default)]
    algorithm: Option<String>,
    #[serde(default)]
    encoding: Option<String>,
    #[serde(default)]
    length: Option<usize>,
    #[serde(default)]
    charset: Option<String>,
}

#[async_trait]
impl Tool for CipherTool {
    fn name(&self) -> String {
        "cipher".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cipher".to_string(),
            description: "Cryptographic operations: hash, encode/decode, password generation, file encryption. Most operations are pure-Rust with no external dependencies.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["hash_text", "hash_file", "encode", "decode", "generate_password", "generate_key",
                                 "encrypt_file", "decrypt_file", "checksum_verify", "info"],
                        "description": "Cryptographic operation"
                    },
                    "text": { "type": "string", "description": "Input text for hash/encode" },
                    "path": { "type": "string", "description": "File path for hash_file/encrypt_file" },
                    "password": { "type": "string", "description": "Password for file encryption" },
                    "output": { "type": "string", "description": "Output file path" },
                    "algorithm": { "type": "string", "description": "Hash algorithm: sha256 (default), sha512, md5" },
                    "encoding": { "type": "string", "description": "Encoding: base64 (default), hex" },
                    "length": { "type": "integer", "description": "Password/key length (default: 24)" },
                    "charset": { "type": "string", "description": "Password charset: full (default), alphanumeric, numeric" }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Hash/encode/password ops are always available (pure Rust). File encryption requires openssl CLI.".into()),
            safety_level: Default::default(),
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: CipherArgs =
            serde_json::from_str(arguments).map_err(|e| Error::ToolArguments {
                tool_name: "cipher".into(),
                message: e.to_string(),
            })?;

        let result = match args.action.as_str() {
            "info" => check_capabilities().await,
            "hash_text" => hash_text(&args),
            "hash_file" => hash_file(&args).await?,
            "encode" => encode(&args),
            "decode" => decode(&args),
            "generate_password" => generate_password(&args),
            "generate_key" => generate_key(&args),
            "encrypt_file" => encrypt_file(&args).await?,
            "decrypt_file" => decrypt_file(&args).await?,
            "checksum_verify" => checksum_verify(&args).await?,
            _ => json!({"error": format!("Unknown action: {}", args.action)}),
        };

        Ok(serde_json::to_string_pretty(&result)?)
    }
}

async fn check_capabilities() -> serde_json::Value {
    let openssl = which::which("openssl").is_ok();
    let degradation = if !openssl {
        ToolDegradation::active(
            "missing_optional_dependency",
            "openssl_cli_unavailable",
            "File encryption is unavailable until openssl is installed.",
            "hash_or_encode_or_install_openssl",
            true,
        )
    } else {
        ToolDegradation::inactive()
    };
    json!({
        "pure_rust": ["hash_text", "hash_file", "encode", "decode", "generate_password", "generate_key", "checksum_verify"],
        "requires_openssl": ["encrypt_file", "decrypt_file"],
        "openssl_available": openssl,
        "degradation": degradation.as_json()
    })
}

// --- Pure Rust operations (always available) ---

fn hash_text(args: &CipherArgs) -> serde_json::Value {
    let algo = args.algorithm.as_deref().unwrap_or("sha256");
    let hash = compute_hash(args.text.as_bytes(), algo);
    json!({"algorithm": algo, "hash": hash, "input_length": args.text.len()})
}

async fn hash_file(args: &CipherArgs) -> anyhow::Result<serde_json::Value> {
    if args.path.is_empty() {
        return Ok(json!({"error": "path is required"}));
    }
    ensure_cipher_file_size(&args.path).await?;
    let data = tokio::fs::read(&args.path).await?;
    let algo = args.algorithm.as_deref().unwrap_or("sha256");
    let hash = compute_hash(&data, algo);
    Ok(json!({"algorithm": algo, "hash": hash, "file": args.path, "size": data.len()}))
}

fn compute_hash(data: &[u8], algo: &str) -> String {
    use sha2::{Digest, Sha256, Sha512};

    match algo {
        "sha256" => hex::encode(Sha256::digest(data)),
        "sha512" => hex::encode(Sha512::digest(data)),
        "md5" => format!("{:x}", md5::compute(data)),
        _ => hex::encode(Sha256::digest(data)),
    }
}

fn encode(args: &CipherArgs) -> serde_json::Value {
    let encoding = args.encoding.as_deref().unwrap_or("base64");
    let result = match encoding {
        "base64" => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(args.text.as_bytes())
        }
        "hex" => hex::encode(args.text.as_bytes()),
        _ => return json!({"error": format!("Unsupported encoding: {}", encoding)}),
    };
    json!({"encoding": encoding, "result": result})
}

fn decode(args: &CipherArgs) -> serde_json::Value {
    let encoding = args.encoding.as_deref().unwrap_or("base64");
    let bytes = match encoding {
        "base64" => {
            use base64::Engine;
            match base64::engine::general_purpose::STANDARD.decode(&args.text) {
                Ok(b) => b,
                Err(e) => return json!({"error": format!("Base64 decode error: {}", e)}),
            }
        }
        "hex" => match hex::decode(&args.text) {
            Ok(b) => b,
            Err(e) => return json!({"error": format!("Hex decode error: {}", e)}),
        },
        _ => return json!({"error": format!("Unsupported encoding: {}", encoding)}),
    };
    let text = String::from_utf8_lossy(&bytes).to_string();
    json!({"encoding": encoding, "result": text})
}

fn generate_password(args: &CipherArgs) -> serde_json::Value {
    let length = args.length.unwrap_or(24);
    let charset = args.charset.as_deref().unwrap_or("full");

    let chars: Vec<char> = match charset {
        "alphanumeric" => "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars().collect(),
        "numeric" => "0123456789".chars().collect(),
        _ => "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()-_=+[]{}|;:,.<>?".chars().collect(),
    };

    let mut rng = rand::rng();
    let password: String = (0..length)
        .map(|_| {
            let idx: usize = rng.random_range(0..chars.len());
            chars[idx]
        })
        .collect();

    let strength = analyze_strength(&password);
    json!({"password": password, "length": length, "charset": charset, "strength": strength})
}

fn analyze_strength(password: &str) -> serde_json::Value {
    let has_upper = password.chars().any(|c| c.is_uppercase());
    let has_lower = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| !c.is_alphanumeric());
    let variety = has_upper as u8 + has_lower as u8 + has_digit as u8 + has_special as u8;
    let score = match (password.len(), variety) {
        (0..=7, _) => "weak",
        (8..=11, 0..=2) => "fair",
        (8..=11, _) => "good",
        (12..=19, 0..=2) => "good",
        (12..=19, _) => "strong",
        (_, _) => "very_strong",
    };
    json!({"score": score, "has_upper": has_upper, "has_lower": has_lower, "has_digit": has_digit, "has_special": has_special})
}

fn generate_key(args: &CipherArgs) -> serde_json::Value {
    let length = args.length.unwrap_or(32);
    let mut rng = rand::rng();
    let key: Vec<u8> = (0..length).map(|_| rng.random()).collect();
    let hex_key = hex::encode(&key);
    json!({"key_hex": hex_key, "length_bytes": length})
}

// --- File encryption (requires openssl CLI, with degradation) ---

async fn encrypt_file(args: &CipherArgs) -> anyhow::Result<serde_json::Value> {
    if args.path.is_empty() || args.password.is_empty() {
        return Ok(json!({"error": "path and password are required"}));
    }
    let output = if args.output.is_empty() {
        format!("{}.enc", args.path)
    } else {
        args.output.clone()
    };

    if which::which("openssl").is_ok() {
        let result = tokio::process::Command::new("openssl")
            .args([
                "enc",
                "-aes-256-cbc",
                "-salt",
                "-pbkdf2",
                "-in",
                &args.path,
                "-out",
                &output,
                "-pass",
                &format!("pass:{}", args.password),
            ])
            .output()
            .await?;

        if result.status.success() {
            Ok(json!({"success": true, "output": output, "method": "aes-256-cbc"}))
        } else {
            Ok(json!({"error": String::from_utf8_lossy(&result.stderr).to_string()}))
        }
    } else {
        Ok(json!({
            "error": "openssl not found — file encryption unavailable",
            "degraded": true,
            "install_hint": "Install openssl: apt install openssl / brew install openssl"
        }))
    }
}

async fn decrypt_file(args: &CipherArgs) -> anyhow::Result<serde_json::Value> {
    if args.path.is_empty() || args.password.is_empty() {
        return Ok(json!({"error": "path and password are required"}));
    }
    let output = if args.output.is_empty() {
        args.path.trim_end_matches(".enc").to_string() + ".dec"
    } else {
        args.output.clone()
    };

    if which::which("openssl").is_ok() {
        let result = tokio::process::Command::new("openssl")
            .args([
                "enc",
                "-d",
                "-aes-256-cbc",
                "-pbkdf2",
                "-in",
                &args.path,
                "-out",
                &output,
                "-pass",
                &format!("pass:{}", args.password),
            ])
            .output()
            .await?;

        if result.status.success() {
            Ok(json!({"success": true, "output": output}))
        } else {
            Ok(json!({"error": String::from_utf8_lossy(&result.stderr).to_string()}))
        }
    } else {
        Ok(json!({
            "error": "openssl not found — file decryption unavailable",
            "degraded": true,
            "install_hint": "Install openssl: apt install openssl / brew install openssl"
        }))
    }
}

async fn checksum_verify(args: &CipherArgs) -> anyhow::Result<serde_json::Value> {
    if args.path.is_empty() || args.text.is_empty() {
        return Ok(json!({"error": "path (file) and text (expected checksum) are required"}));
    }
    ensure_cipher_file_size(&args.path).await?;
    let data = tokio::fs::read(&args.path).await?;
    let algo = args.algorithm.as_deref().unwrap_or("sha256");
    let actual = compute_hash(&data, algo);
    let expected = args.text.to_lowercase();
    Ok(json!({
        "match": actual == expected,
        "algorithm": algo,
        "actual": actual,
        "expected": expected,
    }))
}

async fn ensure_cipher_file_size(path: &str) -> anyhow::Result<()> {
    let metadata = tokio::fs::metadata(path).await?;
    if metadata.len() > MAX_CIPHER_FILE_BYTES {
        anyhow::bail!(
            "file is larger than the 100MB cipher-tool in-memory safety limit: {} bytes",
            metadata.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_definition() {
        let tool = CipherTool;
        let def = tool.definition().await;
        assert_eq!(def.name, "cipher");
    }

    #[test]
    fn test_encode_decode_base64() {
        let args = CipherArgs {
            action: "encode".into(),
            text: "hello world".into(),
            encoding: Some("base64".into()),
            path: String::new(),
            password: String::new(),
            output: String::new(),
            algorithm: None,
            length: None,
            charset: None,
        };
        let encoded = encode(&args);
        assert_eq!(encoded["result"], "aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn test_encode_decode_hex() {
        let args = CipherArgs {
            action: "encode".into(),
            text: "hello".into(),
            encoding: Some("hex".into()),
            path: String::new(),
            password: String::new(),
            output: String::new(),
            algorithm: None,
            length: None,
            charset: None,
        };
        let encoded = encode(&args);
        assert_eq!(encoded["result"], "68656c6c6f");
    }

    #[test]
    fn test_generate_password() {
        let args = CipherArgs {
            action: "generate_password".into(),
            length: Some(20),
            charset: Some("alphanumeric".into()),
            text: String::new(),
            path: String::new(),
            password: String::new(),
            output: String::new(),
            algorithm: None,
            encoding: None,
        };
        let result = generate_password(&args);
        let pw = result["password"].as_str().unwrap();
        assert_eq!(pw.len(), 20);
        assert!(pw.chars().all(|c| c.is_alphanumeric()));
    }

    #[test]
    fn test_password_strength() {
        let strength = analyze_strength("Abc123!@#XyzLong");
        assert_eq!(strength["score"], "strong");
    }

    #[tokio::test]
    async fn test_cipher_info_reports_standard_degradation_object() {
        let info = check_capabilities().await;
        assert_eq!(
            info["degradation"]["schema_version"],
            "benshu.builtin_tools.degradation.v1"
        );
        assert!(info["degradation"]["active"].is_boolean());
        assert!(info["degradation"]["fallback_path"].is_string());
    }
}
