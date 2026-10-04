/// Declares an enum that maps to a DB text column; one string form drives serde, `as_str()`,
/// `parse()` and `Display`, so the wire, the column and Rust cannot drift.
macro_rules! db_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $($variant:ident => $value:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub enum $name {
            $(
                #[serde(rename = $value)]
                $variant,
            )+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The value stored in the database and sent on the wire.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $value),+
                }
            }

            /// Parse a database/wire value; `None` for anything unknown, so callers choose the
            /// fallback rather than mapping it onto a wrong variant.
            pub fn parse(value: &str) -> Option<$name> {
                match value {
                    $($value => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

pub mod category;
pub mod match_thread;
pub mod post;
pub mod user;

pub use category::*;
pub use match_thread::*;
pub use post::*;
pub use user::*;
