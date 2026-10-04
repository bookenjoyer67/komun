use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

db_enum!(
    /// Server-level role; there are no per-community roles.
    Role {
        User => "user",
        Admin => "admin",
        SuperAdmin => "superadmin",
    }
);

impl Role {
    /// Admin routes accept either administrative role; only `SuperAdmin` may change roles.
    pub fn is_admin(&self) -> bool {
        matches!(self, Role::Admin | Role::SuperAdmin)
    }
}

/// A user as other users see them: the public encryption key, never the password verifier or its salt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: Uuid,
    pub display_name: String,
    /// Only populated for the account's owner and for admins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub email_verified: bool,
    pub bio: Option<String>,
    pub avatar_url: Option<String>,
    /// x25519 public key, base64. Used to encrypt messages to this user.
    pub encryption_public_key: Option<String>,
    pub role: Role,
    pub post_count: u32,
    pub verified_post_count: u32,
    pub endorsement_count: u32,
    pub joined_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    #[serde(default)]
    pub profile_json: serde_json::Value,
}

/// The x25519 secret wrapped twice — under a password-derived key and a recovery-code-derived key — plus the salts to re-derive them; the server stores these blobs but holds nothing that can unwrap them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBundle {
    /// x25519 public key, base64.
    pub encryption_public_key: String,
    /// x25519 secret wrapped by the password-derived key, base64.
    pub encrypted_key_bundle: String,
    pub bundle_salt: String,
    /// x25519 secret wrapped by the recovery-code-derived key, base64.
    pub encrypted_recovery_bundle: String,
    pub recovery_bundle_salt: String,
}
