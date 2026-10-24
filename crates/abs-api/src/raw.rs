use std::ops::Deref;

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

/// A decoded value together with the JSON text it came from.
///
/// Only works with serde_json's string and slice deserializers.
#[derive(Debug, Clone)]
pub struct Raw<T> {
    value: T,
    json: Box<RawValue>,
}

impl<T> Raw<T> {
    /// The typed value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// The original JSON text, for storing as `raw_json`.
    pub fn json(&self) -> &str {
        self.json.get()
    }

    /// Splits into the typed value and the JSON text.
    pub fn into_parts(self) -> (T, Box<RawValue>) {
        (self.value, self.json)
    }
}

impl<T> Deref for Raw<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Raw<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let json = Box::<RawValue>::deserialize(deserializer)?;
        let value = serde_json::from_str(json.get()).map_err(D::Error::custom)?;
        Ok(Self { value, json })
    }
}

#[cfg(test)]
mod tests {
    use super::Raw;

    #[derive(serde::Deserialize)]
    struct Item {
        id: String,
    }

    #[test]
    fn keeps_unknown_fields_in_json() {
        let items: Vec<Raw<Item>> = serde_json::from_str(r#"[{"id":"a","extra":1}]"#).unwrap();
        assert_eq!(items[0].id, "a");
        assert_eq!(items[0].json(), r#"{"id":"a","extra":1}"#);
    }
}
