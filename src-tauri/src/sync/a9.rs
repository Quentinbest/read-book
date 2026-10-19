//! A9, SY9, SP12: a highlight's record body, as Linen's own Web Annotation JSON.
//!
//! The same shape `toW3C` writes and `fromW3C` reads (`src/lib/annotations/model.ts`),
//! with two differences sync needs: the target's `source` is the book's sync ID, so no
//! local book ID leaves the device (SY8), and `body` is empty, since the note is a
//! record of its own (`note:`, SY9).

use crate::store::AnnotationRow;
use serde_json::{json, Value};

const LINEN_NS: &str = "https://linen.app/ns/annotation#";
const CFI_SPEC: &str = "http://www.idpf.org/epub/linking/cfi/epub-cfi.html";

/// `Date.prototype.toISOString`'s form of a time in milliseconds since the epoch.
pub fn iso_time(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

/// The inverse of `iso_time`, for the times `record::check_body` accepts.
pub fn parse_iso_time(t: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(
        days * 86_400_000
            + n(11..13)? * 3_600_000
            + n(14..16)? * 60_000
            + n(17..19)? * 1000
            + n(20..23)?,
    )
}

/// A highlight's body, from its row and its book's sync ID.
pub fn body_of(a: &AnnotationRow, book_sync_id: &str) -> Value {
    json!({
        "@context": ["http://www.w3.org/ns/anno.jsonld", { "linen": LINEN_NS }],
        "id": format!("urn:uuid:{}", a.id),
        "type": "Annotation",
        "motivation": "highlighting",
        "created": iso_time(a.created_at),
        "modified": iso_time(a.updated_at),
        "body": [],
        "target": {
            "source": format!("urn:uuid:{book_sync_id}"),
            "selector": [
                { "type": "FragmentSelector", "conformsTo": CFI_SPEC, "value": a.cfi_range },
                { "type": "TextQuoteSelector", "exact": a.quote_exact, "prefix": a.quote_prefix, "suffix": a.quote_suffix },
            ],
        },
        "linen:color": a.color,
        "linen:anchorStatus": a.anchor_status,
        "linen:anchoredContentHash": a.anchored_content_hash,
    })
}

/// The book sync ID a body's target names.
pub fn book_of(body: &Value) -> Option<&str> {
    body["target"]["source"].as_str()?.strip_prefix("urn:uuid:")
}

/// A row from a checked body (`record::check_body` has already validated its shape),
/// for the local book `book_id`. The note is its own record and isn't set here.
pub fn row_of(body: &Value, book_id: &str) -> Option<AnnotationRow> {
    let sel = body["target"]["selector"].as_array()?;
    let first = |kind: &str| sel.iter().find(|x| x["type"] == kind);
    let cfi = first("FragmentSelector")?;
    let quote = first("TextQuoteSelector")?;
    let s = |v: &Value| v.as_str().map(str::to_owned);
    Some(AnnotationRow {
        id: body["id"].as_str()?.strip_prefix("urn:uuid:")?.to_owned(),
        book_id: book_id.to_owned(),
        anchored_content_hash: s(&body["linen:anchoredContentHash"])?,
        color: s(&body["linen:color"])?,
        cfi_range: s(&cfi["value"])?,
        quote_exact: s(&quote["exact"])?,
        quote_prefix: s(&quote["prefix"])?,
        quote_suffix: s(&quote["suffix"])?,
        note: None,
        created_at: parse_iso_time(body["created"].as_str()?)?,
        updated_at: parse_iso_time(body["modified"].as_str()?)?,
        anchor_status: s(&body["linen:anchorStatus"])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::record::{check_body, Key};

    const BOOK: &str = "7c1e1d2a-3b4c-4d5e-8f60-718293a4b5c6";
    const ANNO: &str = "1a2b3c4d-5e6f-4a1b-8c2d-3e4f5a6b7c8d";

    fn row() -> AnnotationRow {
        AnnotationRow {
            id: ANNO.into(),
            book_id: "local-1".into(),
            anchored_content_hash:
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            color: "green".into(),
            cfi_range: "epubcfi(/6/4!/4/2,/1:0,/1:4)".into(),
            quote_exact: "Call".into(),
            quote_prefix: "".into(),
            quote_suffix: " me".into(),
            note: Some("not synced here".into()),
            created_at: 1_760_702_400_123,
            updated_at: 1_760_702_460_000,
            anchor_status: "anchored".into(),
        }
    }

    #[test]
    fn times_round_trip_as_to_iso_string_writes_them() {
        assert_eq!(iso_time(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_time(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(iso_time(1_760_702_400_123), "2025-10-17T12:00:00.123Z");
        for ms in [0, 951_782_400_000, 1_760_702_400_123, 4_102_444_799_999] {
            assert_eq!(parse_iso_time(&iso_time(ms)), Some(ms));
        }
    }

    #[test]
    fn a_highlight_round_trips_through_its_checked_body() {
        let a = row();
        let body = body_of(&a, BOOK);
        // What this device writes passes what every device checks (SY28).
        let key = Key::parse(&format!("annotation:{ANNO}")).unwrap().0;
        assert!(check_body(&key, body.to_string().as_bytes()).is_ok());
        assert_eq!(
            book_of(&body),
            Some(BOOK),
            "the sync ID, never the local ID (SY8)"
        );
        let back = row_of(&body, "local-1").unwrap();
        assert_eq!(
            back,
            AnnotationRow { note: None, ..a },
            "lossless but for the note"
        );
    }
}
