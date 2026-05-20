//! Typed identifiers for core aggregates. Each ID is a transparent newtype
//! over a v4 UUID so values cannot be mixed up at the type level.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! define_id {
    ($name:ident, $prefix:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Generate a new random ID.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wrap an existing UUID. Used by storage layers when hydrating state.
            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            /// Borrow the inner UUID.
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, self.0)
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }
    };
}

define_id!(TabId, "tab", "Identifier for a single tab.");
define_id!(
    WorkspaceId,
    "ws",
    "Identifier for a workspace (group of tabs)."
);
define_id!(
    WidgetId,
    "wgt",
    "Identifier for a side widget (WhatsApp, Telegram, etc)."
);
