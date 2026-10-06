use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub node: NodeConfig,
    pub discovery: DiscoveryConfig,
    pub auth: AuthConfig,
    pub security: SecurityConfig,
    pub posts: PostsConfig,
    pub admin: AdminConfig,
    pub media: MediaConfig,
    pub email: EmailConfig,
    pub registration: RegistrationConfig,
    pub geocode: GeocodeConfig,
    pub market: MarketConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NodeConfig {
    pub name: String,
    pub description: String,
    pub public_url: Option<String>,
    pub location_name: Option<String>,
    pub location_lat: Option<f64>,
    pub location_lon: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DiscoveryConfig {
    pub listed: bool,
    pub directory_url: Option<String>,
    pub directory_enabled: bool,
    /// Whether this server advertises itself as openly registerable, and whether it accepts
    /// peer registrations into its directory. Deliberately SEPARATE from `[registration] mode`,
    /// which governs user signup only.
    pub open_registration: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub token_lifetime_days: u32,
    pub max_registrations_per_hour: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    pub max_posts_per_hour: u32,
    pub max_messages_per_hour: u32,
    pub max_matches_per_hour: u32,
    pub allowed_origins: String,
    /// Reverse proxies whose `X-Forwarded-For` may be believed, as IP literals.
    ///
    /// Empty by default, and that default is the safe one: an unlisted peer's header is ignored
    /// entirely. Honouring the header from anyone would let an attacker reset their own rate
    /// limit by inventing a hop, which is worse than having no limiter, because it looks like one.
    pub trusted_proxies: Vec<String>,
}

impl SecurityConfig {
    /// A typo in this list silently weakens rate limiting, so it is worth a log line.
    pub fn trusted_proxy_ips(&self) -> Vec<std::net::IpAddr> {
        self.trusted_proxies
            .iter()
            .filter_map(|raw| match raw.trim().parse::<std::net::IpAddr>() {
                Ok(ip) => Some(ip),
                Err(_) => {
                    tracing::warn!(
                        "[security] trusted_proxies entry {raw:?} is not an IP address; ignoring it"
                    );
                    None
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PostsConfig {
    pub default_ttl_need_days: u32,
    pub default_ttl_offer_days: u32,
    pub default_ttl_resource_days: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AdminConfig {
    pub superadmin_keys: Vec<String>,
}

/// SMTP settings for verification and password-reset mail.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct EmailConfig {
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    /// From address, e.g. `Komun <noreply@example.org>`.
    pub from: Option<String>,
    pub starttls: bool,
    /// Link base for emailed URLs; falls back to `[node] public_url`.
    pub public_url: Option<String>,
}

impl EmailConfig {
    pub fn is_configured(&self) -> bool {
        self.smtp_host
            .as_deref()
            .is_some_and(|h| !h.trim().is_empty())
            && self.from.as_deref().is_some_and(|f| !f.trim().is_empty())
    }

    pub fn port(&self) -> u16 {
        self.smtp_port.unwrap_or(587)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RegistrationConfig {
    /// `open` | `invite` | `closed`.
    pub mode: String,
    pub require_email_verification: bool,
    pub min_password_length: usize,
}

impl Default for RegistrationConfig {
    fn default() -> Self {
        Self {
            mode: "open".into(),
            require_email_verification: true,
            min_password_length: 12,
        }
    }
}

impl RegistrationConfig {
    pub fn is_valid_mode(&self) -> bool {
        matches!(self.mode.as_str(), "open" | "invite" | "closed")
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MarketConfig {
    /// ISO-4217 alphabetic code used when a market post arrives without a currency of its own.
    ///
    /// **Unset by default, and deliberately so.** There is no currency that is right for an
    /// unconfigured server, and guessing one would silently price every listing on a node in a
    /// unit nobody chose. When this is `None` and a listing carries no currency, the listing
    /// simply has none.
    pub default_currency: Option<String>,
}

impl MarketConfig {
    /// A listing's own currency always wins. The configured default is consulted only
    /// when a market post arrives without one, and when neither exists the post keeps no
    /// currency rather than being given an invented one.
    pub fn resolve_currency(&self, listed: Option<&str>) -> Option<String> {
        match listed {
            Some(code) => Some(code.to_string()),
            None => self.default_currency.clone(),
        }
    }
}

/// An ISO-4217 alphabetic code is exactly three uppercase ASCII letters.
pub fn is_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
}

impl Default for PostsConfig {
    fn default() -> Self {
        Self {
            default_ttl_need_days: 7,
            default_ttl_offer_days: 14,
            default_ttl_resource_days: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    pub avatar_dir: String,
    pub max_avatar_bytes: u64,
    pub post_images_dir: String,
    pub max_post_image_bytes: u64,
    pub max_post_images: u32,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            avatar_dir: "data/avatars".into(),
            max_avatar_bytes: 1_048_576,
            post_images_dir: "data/post-images".into(),
            max_post_image_bytes: 5_242_880,
            max_post_images: 5,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".into(),
            port: 3000,
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgres://komun:komun@localhost:5432/komun".into(),
            max_connections: 20,
        }
    }
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            name: "Komun Node".into(),
            description: "A community local listings server".into(),
            public_url: None,
            location_name: None,
            location_lat: None,
            location_lon: None,
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            // One source of truth: the session layer decides how long a session lives by default.
            token_lifetime_days: crate::sessions::DEFAULT_LIFETIME_DAYS as u32,
            max_registrations_per_hour: 20,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            max_posts_per_hour: 60,
            max_messages_per_hour: 200,
            max_matches_per_hour: 30,
            allowed_origins: "*".into(),
            trusted_proxies: Vec::new(),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = std::env::var("KOMUN_CONFIG").unwrap_or_else(|_| "config.toml".into());

        let mut config = if Path::new(&config_path).exists() {
            let content = std::fs::read_to_string(&config_path)?;
            toml::from_str::<Config>(&content)?
        } else {
            Config::default()
        };

        config.apply_env_overrides();
        config.validate_registration()?;
        config.validate_market()?;
        Ok(config)
    }

    /// Startup fails loudly when verification is demanded but unsendable.
    pub fn validate_registration(&self) -> anyhow::Result<()> {
        if !self.registration.is_valid_mode() {
            return Err(anyhow::anyhow!(
                "[registration] mode must be one of open, invite, closed (got {:?})",
                self.registration.mode
            ));
        }

        if self.registration.require_email_verification && !self.email.is_configured() {
            return Err(anyhow::anyhow!(
                "[registration] require_email_verification is true but [email] is not configured: \
                 set smtp_host and from, or set require_email_verification = false"
            ));
        }

        Ok(())
    }

    /// A malformed `[market] default_currency` is a startup failure, not a surprise at the
    /// first listing.
    ///
    /// The shape check belongs here rather than at the point of use because the alternative is
    /// worse in both directions: a server that accepted `"usd"` would write it into `currency`
    /// and hit `chk_posts_currency` as a 500 on somebody's listing, and a server that quietly
    /// discarded it would price listings in nothing while the operator believed otherwise.
    pub fn validate_market(&self) -> anyhow::Result<()> {
        if let Some(code) = &self.market.default_currency {
            if !is_currency_code(code) {
                return Err(anyhow::anyhow!(
                    "[market] default_currency must be a three-letter uppercase ISO-4217 code \
                     such as \"USD\" (got {code:?}); remove the key to leave it unset"
                ));
            }
        }
        Ok(())
    }

    pub fn open_registration(&self) -> bool {
        self.discovery
            .open_registration
            .unwrap_or(self.registration.mode == "open")
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("KOMUN_BIND_ADDRESS") {
            self.server.bind_address = v;
        }
        if let Ok(v) = std::env::var("KOMUN_PORT") {
            if let Ok(p) = v.parse() {
                self.server.port = p;
            }
        }
        if let Ok(v) = std::env::var("DATABASE_URL") {
            self.database.url = v;
        }
        if let Ok(v) = std::env::var("KOMUN_NODE_NAME") {
            self.node.name = v;
        }
        if let Ok(v) = std::env::var("BIND_ADDR") {
            let parts: Vec<&str> = v.rsplitn(2, ':').collect();
            if parts.len() == 2 {
                if let Ok(p) = parts[0].parse() {
                    self.server.port = p;
                }
                self.server.bind_address = parts[1].into();
            }
        }
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.server.bind_address, self.server.port)
    }

    pub fn public_url(&self) -> String {
        self.node
            .public_url
            .clone()
            .unwrap_or_else(|| format!("http://localhost:{}", self.server.port))
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct GeocodeConfig {
    /// Contact address (email or URL) identifying the operator of this
    /// deployment, sent to Nominatim in the `User-Agent` header.
    ///
    /// Nominatim's usage policy requires the request to identify the
    /// application *and* give a contact a maintainer actually reads, so set
    /// this to a real address. Requests may be blocked when it is missing.
    pub contact: Option<String>,
}

impl GeocodeConfig {
    /// The outbound `User-Agent`: identifies this deployment and carries the
    /// configured contact. Falls back to the historical generic agent when no
    /// contact is configured, so an unconfigured node keeps working.
    pub fn user_agent(&self) -> String {
        match self
            .contact
            .as_deref()
            .map(str::trim)
            .filter(|contact| !contact.is_empty())
        {
            Some(contact) => format!(
                "Komun/{} (+{}; nominatim proxy)",
                env!("CARGO_PKG_VERSION"),
                contact
            ),
            None => concat!(
                "Komun/",
                env!("CARGO_PKG_VERSION"),
                " (nominatim proxy; local-listings app)"
            )
            .to_string(),
        }
    }
}
