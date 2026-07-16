//! Typed entity IDs. ULID strings wrapped in per-entity newtypes so a
//! `PartId` can never be passed where a `ProjectId` is expected.

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("invalid ULID string")]
    InvalidUlid,
}

macro_rules! define_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Generate a fresh ULID-backed id.
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                $name(ulid::Ulid::new().to_string())
            }

            pub fn from_string(s: String) -> Result<Self, IdError> {
                ulid::Ulid::from_string(&s).map_err(|_| IdError::InvalidUlid)?;
                Ok($name(s))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(s: String) -> Result<Self, IdError> {
                Self::from_string(s)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}

define_id!(PartId);
define_id!(VariantId);
define_id!(ListingId);
define_id!(CategoryId);
define_id!(ProjectId);
define_id!(TransactionId);
define_id!(GroupId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_valid_ulids() {
        let id = PartId::new();
        assert_eq!(id.as_str().len(), 26);
        assert!(PartId::from_string(id.as_str().to_string()).is_ok());
    }

    #[test]
    fn invalid_strings_are_rejected() {
        assert!(PartId::from_string("not-a-ulid".into()).is_err());
        assert!(PartId::from_string(String::new()).is_err());
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = PartId::from_string("00000000000000000000000000".into()).unwrap();
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            "\"00000000000000000000000000\""
        );
        let back: PartId = serde_json::from_str("\"00000000000000000000000000\"").unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn deserializing_invalid_id_fails() {
        assert!(serde_json::from_str::<PartId>("\"nope\"").is_err());
    }
}
