//! PS5: caller-owned state saved alongside each completed step.
use super::ids::{DeviceId, Hash32, Id16};
use super::trust::Mark;
use super::{ages, freshness, hold, retention, weeks};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// PS5: JSON object keys cannot represent structured identifiers.
macro_rules! map_module {
    ($name:ident, $map:ident, $bound:path) => {
        pub(crate) mod $name {
            use serde::{Deserialize, Deserializer, Serialize, Serializer};
            use std::collections::$map;
            pub fn serialize<S: Serializer, K: Serialize + $bound, V: Serialize>(
                map: &$map<K, V>,
                s: S,
            ) -> Result<S::Ok, S::Error> {
                map.iter().collect::<Vec<_>>().serialize(s)
            }
            pub fn deserialize<
                'de,
                D: Deserializer<'de>,
                K: Deserialize<'de> + $bound,
                V: Deserialize<'de>,
            >(
                d: D,
            ) -> Result<$map<K, V>, D::Error> {
                Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
            }
        }
    };
}
map_module!(map, BTreeMap, Ord);
// HashMap also needs Eq, supplied by the Hash bound's companion trait below.
pub(crate) mod hash_map {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::{collections::HashMap, hash::Hash};
    pub fn serialize<S: Serializer, K: Serialize + Eq + Hash, V: Serialize>(
        map: &HashMap<K, V>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<
        'de,
        D: Deserializer<'de>,
        K: Deserialize<'de> + Eq + Hash,
        V: Deserialize<'de>,
    >(
        d: D,
    ) -> Result<HashMap<K, V>, D::Error> {
        Ok(Vec::<(K, V)>::deserialize(d)?.into_iter().collect())
    }
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MembershipState {
    #[serde(with = "map")]
    pub first_seen: BTreeMap<Hash32, i64>,
    pub confirmed: BTreeSet<Hash32>,
    pub checkpoint: BTreeSet<Hash32>,
    #[serde(with = "map")]
    pub quarantined: BTreeMap<DeviceId, Option<Mark>>,
    #[serde(with = "map")]
    pub heads: BTreeMap<DeviceId, Option<Hash32>>,
}
#[derive(Debug)]
pub struct State {
    pub ledger: hold::Ledger,
    pub weeks: weeks::KnownWeeks,
    pub heard: freshness::Heard,
    pub marks: ages::Marks,
    pub ages: retention::Ages,
    pub vouched: retention::Vouched,
    pub membership: MembershipState,
}
impl Default for State {
    fn default() -> Self {
        Self {
            ledger: Default::default(),
            weeks: Default::default(),
            heard: Default::default(),
            marks: Default::default(),
            ages: Default::default(),
            vouched: retention::Vouched::new(0),
            membership: Default::default(),
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
fn read<T: DeserializeOwned + Default>(
    conn: &Connection,
    lib: &Id16,
    name: &str,
) -> Result<T, Error> {
    let json: Option<String> = conn
        .query_row(
            "SELECT json FROM sync_pass_state WHERE library = ?1 AND name = ?2",
            params![lib.to_string(), name],
            |r| r.get(0),
        )
        .optional()?;
    json.map(|s| serde_json::from_str(&s).map_err(Error::from))
        .unwrap_or_else(|| Ok(T::default()))
}
fn write<T: Serialize>(
    tx: &Transaction<'_>,
    lib: &Id16,
    name: &str,
    value: &T,
) -> Result<(), Error> {
    tx.execute("INSERT INTO sync_pass_state (library, name, json) VALUES (?1, ?2, ?3) ON CONFLICT(library, name) DO UPDATE SET json = excluded.json", params![lib.to_string(), name, serde_json::to_string(value)?])?;
    Ok(())
}
impl State {
    pub fn load(conn: &Connection, lib: &Id16) -> Result<Self, Error> {
        let json: Option<String> = conn
            .query_row(
                "SELECT json FROM sync_pass_state WHERE library = ?1 AND name = 'vouched'",
                [lib.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(Self {
            ledger: read(conn, lib, "ledger")?,
            weeks: read(conn, lib, "weeks")?,
            heard: read(conn, lib, "heard")?,
            marks: read(conn, lib, "marks")?,
            ages: read(conn, lib, "ages")?,
            vouched: json
                .map(|s| serde_json::from_str(&s))
                .transpose()?
                .unwrap_or_else(|| retention::Vouched::new(0)),
            membership: read(conn, lib, "membership")?,
        })
    }
    pub fn save(&self, tx: &Transaction<'_>, lib: &Id16) -> Result<(), Error> {
        write(tx, lib, "ledger", &self.ledger)?;
        write(tx, lib, "weeks", &self.weeks)?;
        write(tx, lib, "heard", &self.heard)?;
        write(tx, lib, "marks", &self.marks)?;
        write(tx, lib, "ages", &self.ages)?;
        write(tx, lib, "vouched", &self.vouched)?;
        write(tx, lib, "membership", &self.membership)
    }
    /// SY17: resume the same vouched clock used by ages and hearing history.
    pub fn resume(&mut self, mono_ms: u64, wall_ms: u64, clock_ahead: bool, same_boot: bool) {
        self.vouched
            .restart(mono_ms, wall_ms, clock_ahead, same_boot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::sync::record::Entry;
    use serde_json::{json, Value};

    fn setup() -> (Store, Id16) {
        (Store::open_in_memory().unwrap(), Id16([1; 16]))
    }
    fn save(store: &mut Store, lib: &Id16, state: &State) {
        // Store exposes a connection; an unchecked transaction uses the same database.
        let tx = store.conn().unchecked_transaction().unwrap();
        state.save(&tx, lib).unwrap();
        tx.commit().unwrap();
    }
    #[test]
    fn state_round_trips_through_the_database() {
        let (mut store, lib) = setup();
        let d = Id16([2; 16]);
        let h = Hash32([3; 32]);
        let e = Entry {
            device: d,
            seq: 7,
            hash: h,
        };
        let id = json!({"device": d, "seq": 7, "index": 1});
        let change = json!({"device": d, "seq": 7, "hash": h});
        let mark = json!({"s": 7, "h": h});
        let reservation =
            json!({"item":{"kind":"Blob","name":h,"size":9},"first_seen":10,"_claimed":true});
        let holding = json!({"item":{"kind":"Book","name":h,"hash":h,"size":11,"for_key":"setting:theme"},"claimed":true,"change":change,"accepted_at":12,"naming":[change,e]});
        let ledger =
            json!({"reservations":[[id,reservation]],"holds":[[id,holding]],"last":[[d,mark]]});
        let weeks = json!({"entries":[[e,[["setting:theme",[[{"kind":"Blob","name":h},true]]]]]]});
        let heard = json!({"counters":[[d,[3,4]]],"applied":[[d,5]]});
        let marks = json!({"own":[[e,6]],"in_view":[e],"reported":[[e,[d]]]});
        let ages = json!({"first_seen":[[e,8]],"reported":[[[e,d],[9,10]]]});
        let vouched =
            json!({"total_ms":100,"last_mono_ms":42,"last_wall_ms":300,"untrusted_wall":true});
        let membership = json!({"first_seen":[[h,-2]],"confirmed":[h],"checkpoint":[h],"quarantined":[[d,mark]],"heads":[[d,h]]});
        let state = State {
            ledger: serde_json::from_value(ledger.clone()).unwrap(),
            weeks: serde_json::from_value(weeks.clone()).unwrap(),
            heard: serde_json::from_value(heard.clone()).unwrap(),
            marks: serde_json::from_value(marks.clone()).unwrap(),
            ages: serde_json::from_value(ages.clone()).unwrap(),
            vouched: serde_json::from_value(vouched.clone()).unwrap(),
            membership: serde_json::from_value(membership.clone()).unwrap(),
        };
        save(&mut store, &lib, &state);
        let loaded = State::load(store.conn(), &lib).unwrap();
        for (actual, expected) in [
            (serde_json::to_value(loaded.ledger).unwrap(), ledger),
            (serde_json::to_value(loaded.weeks).unwrap(), weeks),
            (serde_json::to_value(loaded.heard).unwrap(), heard),
            (serde_json::to_value(loaded.marks).unwrap(), marks),
            (serde_json::to_value(loaded.ages).unwrap(), ages),
            (serde_json::to_value(loaded.vouched).unwrap(), vouched),
            (serde_json::to_value(loaded.membership).unwrap(), membership),
        ] {
            assert_eq!(actual, expected);
        }
    }
    #[test]
    fn a_restart_keeps_a_deletions_age_and_a_witness_timeout() {
        let (mut store, lib) = setup();
        let mut state = State::default();
        let d = Id16([2; 16]);
        let e = Entry {
            device: d,
            seq: 1,
            hash: Hash32([3; 32]),
        };
        state.vouched.tick(100, 1000);
        state.vouched.tick(200, 1100);
        state.ages.saw(e, state.vouched.now());
        state.heard.saw_counter(d, 1, &state.vouched);
        state.vouched.tick(300, 1200);
        save(&mut store, &lib, &state);
        for same_boot in [true, false] {
            let mut loaded = State::load(store.conn(), &lib).unwrap();
            loaded.resume(if same_boot { 400 } else { 10 }, 1300, false, same_boot);
            assert_eq!(loaded.ages.own_age(&e, loaded.vouched.now()), Some(200));
            let heard: Value = serde_json::to_value(&loaded.heard).unwrap();
            assert_eq!(heard["counters"][0][1][1], 100);
            assert_eq!(
                loaded.vouched.now() - heard["counters"][0][1][1].as_u64().unwrap(),
                200
            );
        }
    }
    #[test]
    fn membership_inputs_survive_a_restart() {
        let (mut store, lib) = setup();
        let mut state = State::default();
        let h = Hash32([9; 32]);
        state.membership.first_seen.insert(h, 123);
        save(&mut store, &lib, &state);
        assert_eq!(
            State::load(store.conn(), &lib)
                .unwrap()
                .membership
                .first_seen
                .get(&h),
            Some(&123)
        );
    }
}
