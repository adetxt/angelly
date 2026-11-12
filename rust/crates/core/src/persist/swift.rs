//! Helpers that reproduce how Swift's synthesized `Codable` lays out values, so
//! files written by the macOS app stay readable and vice versa.

use std::time::SystemTime;

use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserializer, Serializer};

use crate::dates;

/// Seconds between the Unix epoch and Foundation's reference date (2001-01-01).
const REFERENCE_DATE_OFFSET: f64 = 978_307_200.0;

struct EmptyObject;

impl serde::Serialize for EmptyObject {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_map(Some(0))?.end()
    }
}

pub(crate) fn serialize_case<S: Serializer>(serializer: S, key: &str) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry(key, &EmptyObject)?;
    map.end()
}

pub(crate) fn deserialize_case<'de, D, T>(
    deserializer: D,
    parse: fn(&str) -> Option<T>,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
{
    struct CaseVisitor<T>(fn(&str) -> Option<T>);

    impl<'de, T> Visitor<'de> for CaseVisitor<T> {
        type Value = T;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("an object with exactly one case key")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
            let key: String = map
                .next_key()?
                .ok_or_else(|| de::Error::custom("expected exactly one case key"))?;
            let value = (self.0)(&key).ok_or_else(|| de::Error::unknown_variant(&key, &[]))?;
            map.next_value::<IgnoredAny>()?;
            if map.next_key::<IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("expected exactly one case key"));
            }
            Ok(value)
        }
    }

    deserializer.deserialize_map(CaseVisitor(parse))
}

/// Declares an enum whose cases carry no payload, encoded the way Swift does:
/// `{"caseName":{}}`.
macro_rules! swift_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident { $($variant:ident => $key:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        $vis enum $name { $($variant),+ }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let key = match self { $(Self::$variant => $key),+ };
                $crate::persist::swift::serialize_case(serializer, key)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                $crate::persist::swift::deserialize_case(deserializer, |key| match key {
                    $($key => Some(Self::$variant),)+
                    _ => None,
                })
            }
        }
    };
}
pub(crate) use swift_enum;

/// UUIDs encode as uppercase hyphenated strings.
pub(crate) mod uuid {
    use serde::{de, Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S: Serializer>(id: &Uuid, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&string(id))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Uuid, D::Error> {
        let text = String::deserialize(deserializer)?;
        Uuid::parse_str(&text).map_err(de::Error::custom)
    }

    pub fn string(id: &Uuid) -> String {
        id.hyphenated().to_string().to_uppercase()
    }
}

/// `Date` encodes as seconds since 2001-01-01.
pub(crate) mod date {
    use super::*;
    use serde::Deserialize;

    pub fn serialize<S: Serializer>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(dates::to_unix_secs(*time) - REFERENCE_DATE_OFFSET)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
        let seconds = f64::deserialize(deserializer)?;
        dates::from_unix_secs(seconds + REFERENCE_DATE_OFFSET)
            .ok_or_else(|| de::Error::custom("date out of range"))
    }
}

/// `Data` encodes as a padded base64 string.
pub(crate) mod data {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{de, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(de::Error::custom)
    }
}
