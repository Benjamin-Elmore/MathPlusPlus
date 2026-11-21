//! Panes point at open documents by number, but a saved layout must outlive
//! the numbers. While a layout is written or read, this table turns a number
//! into the file's path and back.

use std::cell::RefCell;

use serde::{Deserialize, Deserializer, Serializer};

thread_local! {
    static TABLE: RefCell<Vec<(u64, String)>> = const { RefCell::new(Vec::new()) };
}

/// Runs `f` with `table` (document number, key) installed.
pub fn with_table<R>(table: Vec<(u64, String)>, f: impl FnOnce() -> R) -> R {
    TABLE.with(|t| *t.borrow_mut() = table);
    let out = f();
    TABLE.with(|t| t.borrow_mut().clear());
    out
}

/// Serializes a document number as its key.
pub fn serialize<S: Serializer>(id: &u64, s: S) -> Result<S::Ok, S::Error> {
    let key = TABLE.with(|t| {
        t.borrow()
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, k)| k.clone())
    });
    s.serialize_str(&key.unwrap_or_default())
}

/// Reads a key back as a document number (0 when that file is not open).
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let key = String::deserialize(d)?;
    Ok(TABLE.with(|t| {
        t.borrow()
            .iter()
            .find(|(_, k)| *k == key)
            .map_or(0, |(i, _)| *i)
    }))
}
