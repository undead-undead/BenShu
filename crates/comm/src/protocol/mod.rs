pub mod a2a;
pub mod b2h;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/benshu.comm.rs"));
}

use crate::error::ProtocolError;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

/// Unified Addressing System
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Address {
    /// Internal agent communication (agent://<id>)
    Agent(String),
    /// Human interaction (user://<id>)
    User(String),
    /// System broadcast (system://all)
    System(String),
}

impl Address {
    /// Check if target is user
    pub fn is_user(&self) -> bool {
        matches!(self, Self::User(_))
    }

    /// Check if target is agent
    pub fn is_agent(&self) -> bool {
        matches!(self, Self::Agent(_))
    }

    /// Get inner ID string
    pub fn id(&self) -> &str {
        match self {
            Self::Agent(id) => id,
            Self::User(id) => id,
            Self::System(id) => id,
        }
    }

    /// Check if this is a fractal (hierarchical) address
    pub fn is_fractal(&self) -> bool {
        match self {
            Self::Agent(id) => id.contains('/'),
            _ => false,
        }
    }

    /// Get hierarchy levels
    pub fn hierarchy(&self) -> Vec<&str> {
        self.id().split('/').collect()
    }

    /// Get parent address if hierarchical
    pub fn parent(&self) -> Option<Address> {
        if !self.is_fractal() {
            return None;
        }

        match self {
            Self::Agent(id) => {
                let parts: Vec<&str> = id.split('/').collect();
                if parts.len() > 1 {
                    let parent_id = parts[..parts.len() - 1].join("/");
                    Some(Address::Agent(parent_id))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get the root agent ID if hierarchical
    pub fn root_id(&self) -> &str {
        self.id().split('/').next().unwrap_or(self.id())
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Agent(id) => write!(f, "agent://{}", id),
            Self::User(id) => write!(f, "user://{}", id),
            Self::System(id) => write!(f, "system://{}", id),
        }
    }
}

impl FromStr for Address {
    type Err = ProtocolError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        if let Some(id) = s.strip_prefix("agent://") {
            Ok(Self::Agent(id.to_string()))
        } else if let Some(id) = s.strip_prefix("user://") {
            Ok(Self::User(id.to_string()))
        } else if let Some(id) = s.strip_prefix("system://") {
            Ok(Self::System(id.to_string()))
        } else {
            Err(ProtocolError::InvalidAddress(format!(
                "Unknown address prefix: {}",
                s
            )))
        }
    }
}

/// Message Metadata
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalityMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_message_id: Option<String>,
}

impl CausalityMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    pub fn with_task_lineage(
        mut self,
        task_id: impl Into<String>,
        parent_task_id: Option<String>,
        root_task_id: Option<String>,
    ) -> Self {
        self.task_id = Some(task_id.into());
        self.parent_task_id = parent_task_id;
        self.root_task_id = root_task_id;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    /// Unique message identifier
    pub id: Uuid,
    /// Source of the message
    pub source: Address,
    /// Priority level (higher is more urgent)
    pub priority: u32,
    /// Creation timestamp (Unix epoch duration)
    pub timestamp: Duration,
    /// Message timeout duration
    pub timeout: Option<Duration>,
    /// Security signature for verification
    pub signature: Option<String>,
    /// Address of the entity that signed this message
    pub signer: Option<Address>,
    /// Logical tenant identifier for isolation
    pub tenant_id: Option<String>,
    /// Cross-envelope causality for durable routing and tracing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causality: Option<CausalityMetadata>,
}

impl Metadata {
    /// Create new default metadata with source
    pub fn new(source: Address) -> Self {
        Self {
            id: Uuid::new_v4(),
            source,
            priority: 0,
            timestamp: Duration::from_secs(chrono::Utc::now().timestamp() as u64),
            timeout: Some(Duration::from_secs(30)),
            signature: None,
            signer: None,
            tenant_id: None,
            causality: None,
        }
    }

    pub fn with_causality(mut self, causality: CausalityMetadata) -> Self {
        self.causality = Some(causality);
        self
    }

    /// Generate payload for signing
    fn signing_payload(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(self.id.as_bytes());
        payload.extend_from_slice(self.source.to_string().as_bytes());
        payload.extend_from_slice(&self.timestamp.as_secs().to_le_bytes());
        if let Some(tid) = &self.tenant_id {
            payload.extend_from_slice(tid.as_bytes());
        }
        payload
    }

    /// Sign the metadata using a secret key
    pub fn sign(&mut self, key: &[u8], signer_addr: Address) -> Result<(), ProtocolError> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(key)
            .map_err(|e| ProtocolError::Validation(format!("Invalid key length: {}", e)))?;

        mac.update(&self.signing_payload());
        let result = mac.finalize();
        let code_bytes = result.into_bytes();

        // Manual hex conversion
        let mut hex_sig = String::with_capacity(code_bytes.len() * 2);
        for byte in code_bytes {
            hex_sig.push_str(&format!("{:02x}", byte));
        }

        self.signature = Some(hex_sig);
        self.signer = Some(signer_addr);
        Ok(())
    }

    /// Verify the metadata signature
    pub fn verify(&self, key: &[u8]) -> bool {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let (sig_hex, _signer) = match (&self.signature, &self.signer) {
            (Some(s), Some(addr)) => (s, addr),
            _ => return false,
        };

        let mut mac = match HmacSha256::new_from_slice(key) {
            Ok(m) => m,
            Err(_) => return false,
        };

        mac.update(&self.signing_payload());

        // Decode hex
        let mut sig_bytes = Vec::new();
        for i in (0..sig_hex.len()).step_by(2) {
            if let Ok(byte) = u8::from_str_radix(&sig_hex[i..i + 2], 16) {
                sig_bytes.push(byte);
            } else {
                return false;
            }
        }

        mac.verify_slice(&sig_bytes).is_ok()
    }
}

/// Unified Communication Envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommEnvelope {
    /// Destination address
    pub target: Address,
    /// Serialized message payload
    pub payload: Vec<u8>,
    /// Metadata
    pub meta: Metadata,
}

impl From<Address> for proto::Address {
    fn from(addr: Address) -> Self {
        use proto::address::AddrType;
        match addr {
            Address::Agent(id) => Self {
                addr_type: Some(AddrType::Agent(id)),
            },
            Address::User(id) => Self {
                addr_type: Some(AddrType::User(id)),
            },
            Address::System(id) => Self {
                addr_type: Some(AddrType::System(id)),
            },
        }
    }
}

impl From<proto::Address> for Address {
    fn from(p: proto::Address) -> Self {
        use proto::address::AddrType;
        match p.addr_type {
            Some(AddrType::Agent(id)) => Address::Agent(id),
            Some(AddrType::User(id)) => Address::User(id),
            Some(AddrType::System(id)) => Address::System(id),
            None => Address::System("unknown".to_string()),
        }
    }
}

impl From<Metadata> for proto::Metadata {
    fn from(m: Metadata) -> Self {
        Self {
            id: m.id.to_string(),
            source: Some(m.source.into()),
            priority: m.priority,
            timestamp: m.timestamp.as_secs(),
            timeout: m.timeout.map(|d| d.as_secs()).unwrap_or(0),
            signature: m.signature.unwrap_or_default(),
            signer: m.signer.map(Into::into),
            tenant_id: m.tenant_id.unwrap_or_default(),
            session_id: m
                .causality
                .as_ref()
                .and_then(|c| c.session_id.clone())
                .unwrap_or_default(),
            trace_id: m
                .causality
                .as_ref()
                .and_then(|c| c.trace_id.clone())
                .unwrap_or_default(),
            task_id: m
                .causality
                .as_ref()
                .and_then(|c| c.task_id.clone())
                .unwrap_or_default(),
            parent_task_id: m
                .causality
                .as_ref()
                .and_then(|c| c.parent_task_id.clone())
                .unwrap_or_default(),
            root_task_id: m
                .causality
                .as_ref()
                .and_then(|c| c.root_task_id.clone())
                .unwrap_or_default(),
            parent_message_id: m
                .causality
                .as_ref()
                .and_then(|c| c.parent_message_id.clone())
                .unwrap_or_default(),
            root_message_id: m
                .causality
                .as_ref()
                .and_then(|c| c.root_message_id.clone())
                .unwrap_or_default(),
        }
    }
}

impl From<proto::Metadata> for Metadata {
    fn from(p: proto::Metadata) -> Self {
        let causality = CausalityMetadata {
            session_id: (!p.session_id.is_empty()).then_some(p.session_id),
            trace_id: (!p.trace_id.is_empty()).then_some(p.trace_id),
            task_id: (!p.task_id.is_empty()).then_some(p.task_id),
            parent_task_id: (!p.parent_task_id.is_empty()).then_some(p.parent_task_id),
            root_task_id: (!p.root_task_id.is_empty()).then_some(p.root_task_id),
            parent_message_id: (!p.parent_message_id.is_empty()).then_some(p.parent_message_id),
            root_message_id: (!p.root_message_id.is_empty()).then_some(p.root_message_id),
        };
        Self {
            id: Uuid::parse_str(&p.id).unwrap_or_default(),
            source: p
                .source
                .map(Address::from)
                .unwrap_or_else(|| Address::System("unknown".to_string())),
            priority: p.priority,
            timestamp: Duration::from_secs(p.timestamp),
            timeout: if p.timeout > 0 {
                Some(Duration::from_secs(p.timeout))
            } else {
                None
            },
            signature: if p.signature.is_empty() {
                None
            } else {
                Some(p.signature)
            },
            signer: p.signer.map(Address::from),
            tenant_id: if p.tenant_id.is_empty() {
                None
            } else {
                Some(p.tenant_id)
            },
            causality: if causality == CausalityMetadata::default() {
                None
            } else {
                Some(causality)
            },
        }
    }
}

impl From<CommEnvelope> for proto::CommEnvelope {
    fn from(e: CommEnvelope) -> Self {
        Self {
            target: Some(e.target.into()),
            payload: e.payload,
            meta: Some(e.meta.into()),
        }
    }
}

impl From<proto::CommEnvelope> for CommEnvelope {
    fn from(p: proto::CommEnvelope) -> Self {
        Self {
            target: p
                .target
                .map(Address::from)
                .unwrap_or_else(|| Address::System("unknown".to_string())),
            payload: p.payload,
            meta: p
                .meta
                .map(Metadata::from)
                .unwrap_or_else(|| Metadata::new(Address::System("unknown".to_string()))),
        }
    }
}

impl CommEnvelope {
    /// Create a new envelope
    pub fn new(target: Address, payload: Vec<u8>, meta: Metadata) -> Self {
        Self {
            target,
            payload,
            meta,
        }
    }

    pub fn new_with_source(target: Address, payload: Vec<u8>, source: Address) -> Self {
        Self::new(target, payload, Metadata::new(source))
    }

    pub fn with_causality(mut self, causality: CausalityMetadata) -> Self {
        self.meta = self.meta.with_causality(causality);
        self
    }

    pub fn link_to_parent(mut self, parent: &CommEnvelope) -> Self {
        let parent_id = parent.meta.id.to_string();
        let mut causality = self.meta.causality.clone().unwrap_or_default();
        let parent_causality = parent.meta.causality.as_ref();
        if causality.session_id.is_none() {
            causality.session_id = parent_causality.and_then(|c| c.session_id.clone());
        }
        if causality.trace_id.is_none() {
            causality.trace_id = parent_causality.and_then(|c| c.trace_id.clone());
        }
        if causality.parent_task_id.is_none() {
            causality.parent_task_id = parent_causality.and_then(|c| c.task_id.clone());
        }
        if causality.root_task_id.is_none() {
            causality.root_task_id = parent_causality
                .and_then(|c| c.root_task_id.clone())
                .or_else(|| parent_causality.and_then(|c| c.task_id.clone()));
        }
        causality.parent_message_id = Some(parent_id.clone());
        causality.root_message_id = parent_causality
            .and_then(|c| c.root_message_id.clone())
            .or(Some(parent_id));
        self.meta.causality = Some(causality);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_parsing() {
        let addr: Address = "agent://coordinator-1".parse().unwrap();
        assert_eq!(addr, Address::Agent("coordinator-1".to_string()));
        assert_eq!(addr.to_string(), "agent://coordinator-1");

        let user_addr: Address = "user://alice".parse().unwrap();
        assert_eq!(user_addr, Address::User("alice".to_string()));
        assert!(user_addr.is_user());

        let err = "invalid://something".parse::<Address>();
        assert!(err.is_err());
    }

    #[test]
    fn test_fractal_addressing() {
        let addr: Address = "agent://parent/child/grandchild".parse().unwrap();
        assert!(addr.is_fractal());
        assert_eq!(addr.hierarchy(), vec!["parent", "child", "grandchild"]);
        assert_eq!(addr.root_id(), "parent");

        let parent = addr.parent().unwrap();
        assert_eq!(parent.to_string(), "agent://parent/child");
        assert!(parent.is_fractal());

        let root = parent.parent().unwrap();
        assert_eq!(root.to_string(), "agent://parent");
        assert!(!root.is_fractal());
        assert!(root.parent().is_none());
    }

    #[test]
    fn test_envelope_creation() {
        let from = Address::Agent("a1".to_string());
        let to = Address::User("u1".to_string());
        let payload = b"hello".to_vec();
        let meta = Metadata::new(from.clone());

        let envelope = CommEnvelope::new(to.clone(), payload.clone(), meta);

        assert_eq!(envelope.target, to);
        assert_eq!(envelope.payload, payload);
    }

    #[test]
    fn test_protobuf_roundtrip() {
        use prost::Message;

        let from = Address::Agent("a1".to_string());
        let to = Address::User("u1".to_string());
        let payload = b"hello".to_vec();
        let meta = Metadata::new(from.clone());
        let envelope = CommEnvelope::new(to.clone(), payload.clone(), meta);

        // Convert to proto
        let p_envelope: proto::CommEnvelope = envelope.clone().into();

        // Encode
        let mut buf = Vec::new();
        p_envelope.encode(&mut buf).unwrap();

        // Decode
        let decoded_p = proto::CommEnvelope::decode(&buf[..]).unwrap();

        // Convert back
        let decoded: CommEnvelope = decoded_p.into();

        assert_eq!(decoded.target, envelope.target);
        assert_eq!(decoded.payload, envelope.payload);
        assert_eq!(decoded.meta.id, envelope.meta.id);
        assert_eq!(decoded.meta.source, envelope.meta.source);
    }

    #[test]
    fn test_causality_roundtrip_and_parent_linking() {
        use prost::Message;

        let parent = CommEnvelope::new_with_source(
            Address::Agent("child".to_string()),
            b"parent".to_vec(),
            Address::Agent("root".to_string()),
        )
        .with_causality(
            CausalityMetadata::new()
                .with_session_id("session-1")
                .with_trace_id("trace-1")
                .with_task_lineage("task-parent", None, Some("task-root".to_string())),
        );

        let child = CommEnvelope::new_with_source(
            Address::Agent("leaf".to_string()),
            b"child".to_vec(),
            Address::Agent("child".to_string()),
        )
        .with_causality(CausalityMetadata::new().with_task_lineage("task-child", None, None))
        .link_to_parent(&parent);

        let causality = child.meta.causality.as_ref().expect("causality");
        assert_eq!(causality.session_id.as_deref(), Some("session-1"));
        assert_eq!(causality.trace_id.as_deref(), Some("trace-1"));
        assert_eq!(causality.parent_task_id.as_deref(), Some("task-parent"));
        assert_eq!(causality.root_task_id.as_deref(), Some("task-root"));
        assert_eq!(
            causality.parent_message_id.as_deref(),
            Some(parent.meta.id.to_string().as_str())
        );

        let proto_envelope: proto::CommEnvelope = child.clone().into();
        let mut buf = Vec::new();
        proto_envelope.encode(&mut buf).unwrap();
        let decoded = proto::CommEnvelope::decode(&buf[..]).unwrap();
        let roundtrip: CommEnvelope = decoded.into();
        let decoded_causality = roundtrip.meta.causality.expect("decoded causality");
        assert_eq!(decoded_causality.trace_id.as_deref(), Some("trace-1"));
        assert_eq!(decoded_causality.task_id.as_deref(), Some("task-child"));
    }

    #[test]
    fn test_metadata_signing() {
        let source = Address::Agent("a1".to_string());
        let mut meta = Metadata::new(source.clone());
        let secret = b"super-secret-key-that-must-be-32-bytes-long-plus-padding-x";

        // Sign
        meta.sign(secret, source.clone()).unwrap();
        assert!(meta.signature.is_some());
        assert_eq!(meta.signer, Some(source));

        // Verify Correct
        assert!(meta.verify(secret));

        // Verify Wrong Key
        assert!(!meta.verify(b"wrong-key"));

        // Verify Tampered Data
        let mut tampered = meta.clone();
        tampered.timestamp += Duration::from_secs(1);
        assert!(!tampered.verify(secret));
    }
}
