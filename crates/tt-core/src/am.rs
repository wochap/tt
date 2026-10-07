//! Small typed helpers over the Automerge read/write API.
//!
//! Strings users edit are stored as Automerge `Text` objects so JS clients see
//! plain strings and concurrent edits merge character-wise; readers accept
//! scalar strings too. Timestamps are integer milliseconds since the Unix
//! epoch, UTC.

use automerge::{
    AutomergeError, ObjId, ObjType, ROOT, ReadDoc, ScalarValue, Value, transaction::Transactable,
};
use chrono::{DateTime, TimeZone, Utc};

pub type AmResult<T> = Result<T, AutomergeError>;

pub fn root() -> ObjId {
    ROOT
}

/// Map child object, if present and of the right type.
pub fn get_map<D: ReadDoc>(doc: &D, obj: &ObjId, key: &str) -> Option<ObjId> {
    match doc.get(obj, key).ok()?? {
        (Value::Object(ObjType::Map), id) => Some(id),
        _ => None,
    }
}

pub fn get_string<D: ReadDoc>(doc: &D, obj: &ObjId, key: &str) -> Option<String> {
    match doc.get(obj, key).ok()?? {
        (Value::Object(ObjType::Text), id) => doc.text(&id).ok(),
        (Value::Scalar(scalar), _) => match scalar.as_ref() {
            ScalarValue::Str(value) => Some(value.to_string()),
            _ => None,
        },
        _ => None,
    }
}

pub fn get_i64<D: ReadDoc>(doc: &D, obj: &ObjId, key: &str) -> Option<i64> {
    match doc.get(obj, key).ok()?? {
        (Value::Scalar(scalar), _) => match scalar.as_ref() {
            ScalarValue::Int(value) | ScalarValue::Timestamp(value) => Some(*value),
            ScalarValue::Uint(value) => i64::try_from(*value).ok(),
            ScalarValue::F64(value) => Some(*value as i64),
            ScalarValue::Counter(counter) => Some(i64::from(counter)),
            _ => None,
        },
        _ => None,
    }
}

pub fn get_bool<D: ReadDoc>(doc: &D, obj: &ObjId, key: &str) -> Option<bool> {
    match doc.get(obj, key).ok()?? {
        (Value::Scalar(scalar), _) => match scalar.as_ref() {
            ScalarValue::Boolean(value) => Some(*value),
            _ => None,
        },
        _ => None,
    }
}

pub fn get_time<D: ReadDoc>(doc: &D, obj: &ObjId, key: &str) -> Option<DateTime<Utc>> {
    get_i64(doc, obj, key).and_then(from_millis)
}

pub fn keys<D: ReadDoc>(doc: &D, obj: &ObjId) -> Vec<String> {
    doc.keys(obj).collect()
}

pub fn from_millis(value: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(value).single()
}

pub fn millis(value: DateTime<Utc>) -> i64 {
    value.timestamp_millis()
}

/// Returns the map at `key`, creating it when absent.
pub fn ensure_map<T: Transactable + ReadDoc>(
    tx: &mut T,
    obj: &ObjId,
    key: &str,
) -> AmResult<ObjId> {
    match get_map(tx, obj, key) {
        Some(id) => Ok(id),
        None => tx.put_object(obj, key, ObjType::Map),
    }
}

/// Writes a string as a Text object, editing an existing one in place.
pub fn put_text<T: Transactable + ReadDoc>(
    tx: &mut T,
    obj: &ObjId,
    key: &str,
    value: &str,
) -> AmResult<()> {
    if let Some((Value::Object(ObjType::Text), id)) = tx.get(obj, key)? {
        if tx.text(&id)? != value {
            tx.update_text(&id, value)?;
        }
        return Ok(());
    }
    let id = tx.put_object(obj, key, ObjType::Text)?;
    if !value.is_empty() {
        tx.splice_text(&id, 0, 0, value)?;
    }
    Ok(())
}

/// Writes or deletes an optional string.
pub fn put_opt_text<T: Transactable + ReadDoc>(
    tx: &mut T,
    obj: &ObjId,
    key: &str,
    value: Option<&str>,
) -> AmResult<()> {
    match value {
        Some(value) => put_text(tx, obj, key, value),
        None => delete_if_present(tx, obj, key),
    }
}

pub fn put_time<T: Transactable>(
    tx: &mut T,
    obj: &ObjId,
    key: &str,
    value: DateTime<Utc>,
) -> AmResult<()> {
    tx.put(obj, key, millis(value))
}

pub fn put_opt_time<T: Transactable + ReadDoc>(
    tx: &mut T,
    obj: &ObjId,
    key: &str,
    value: Option<DateTime<Utc>>,
) -> AmResult<()> {
    match value {
        Some(value) => put_time(tx, obj, key, value),
        None => {
            // `null` rather than absence keeps "running" explicit for JS readers.
            let is_null =
                matches!(tx.get(obj, key)?, Some((Value::Scalar(scalar), _)) if scalar.is_null());
            if !is_null {
                tx.put(obj, key, ScalarValue::Null)?;
            }
            Ok(())
        }
    }
}

pub fn delete_if_present<T: Transactable + ReadDoc>(
    tx: &mut T,
    obj: &ObjId,
    key: &str,
) -> AmResult<()> {
    if tx.get(obj, key)?.is_some() {
        tx.delete(obj, key)?;
    }
    Ok(())
}
