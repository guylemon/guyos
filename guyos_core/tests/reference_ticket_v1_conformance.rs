//! Golden vectors from `docs/fixtures/0004-ticket-profile-reference-v1.json` (ADR 0004 **G3**).

use guyos_core::{ReferenceTicketError, decode_reference_v1, encode_reference_v1};
use serde::Deserialize;

const FIXTURE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/fixtures/0004-ticket-profile-reference-v1.json"
));

#[derive(Debug, Deserialize)]
struct Fixture {
    #[allow(dead_code)]
    fixture_schema_version: u64,
    #[allow(dead_code)]
    profile_id: String,
    vectors: Vec<Vector>,
}

#[derive(Debug, Deserialize)]
struct Vector {
    name: String,
    ticket: String,
    key_hex: String,
    now_unix: u64,
    expect: Expect,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Expect {
    Ok { room_id: String },
    Error { code: String },
}

#[test]
fn reference_v1_fixture_vectors() {
    let fixture: Fixture = serde_json::from_str(FIXTURE_JSON).expect("parse fixture JSON");

    assert_eq!(fixture.profile_id, "guyos.reference_v1");

    const REQUIRED_NAMES: &[&str] = &[
        "ok_no_expiry",
        "ok_with_expiry",
        "ok_expiry_boundary",
        "expired",
        "bad_signature",
        "wrong_version",
        "wrong_length_short",
        "wrong_length_long",
        "invalid_base64url_padding",
        "invalid_base64url_alphabet",
        "truncated_base64url",
    ];
    for &name in REQUIRED_NAMES {
        assert!(
            fixture.vectors.iter().any(|v| v.name == name),
            "fixture missing required vector {name}"
        );
    }

    for v in &fixture.vectors {
        let key_bytes = hex::decode(&v.key_hex).expect("hex key");
        let key: [u8; 32] = key_bytes.as_slice().try_into().expect("key length");

        let got = decode_reference_v1(&v.ticket, &key, v.now_unix);

        match &v.expect {
            Expect::Ok { room_id } => match got {
                Ok(id) => assert_eq!(id, *room_id, "vector {}", v.name),
                Err(e) => panic!("vector {}: expected Ok, got {:?}", v.name, e),
            },
            Expect::Error { code } => match got {
                Ok(id) => panic!("vector {}: expected error {}, got Ok({})", v.name, code, id),
                Err(e) => assert_eq!(e.portable_code(), code.as_str(), "vector {}", v.name),
            },
        }
    }
}

#[test]
fn ok_no_expiry_encode_round_trip_matches_golden_ticket() {
    let fixture: Fixture = serde_json::from_str(FIXTURE_JSON).expect("parse fixture JSON");
    let v = fixture
        .vectors
        .iter()
        .find(|x| x.name == "ok_no_expiry")
        .expect("ok_no_expiry vector");

    let key: [u8; 32] = hex::decode(&v.key_hex)
        .expect("hex key")
        .try_into()
        .expect("key len");

    let room_str = decode_reference_v1(&v.ticket, &key, v.now_unix).expect("decode");
    let compact: String = room_str.chars().filter(|c| *c != '-').collect();
    let room: [u8; 16] = hex::decode(compact)
        .expect("room hex")
        .try_into()
        .expect("room len");

    assert_eq!(encode_reference_v1(&key, &room, 0), v.ticket);
}

#[test]
fn error_enum_matches_portable_codes() {
    assert_eq!(
        ReferenceTicketError::TicketDecodeFailed.portable_code(),
        "ticket_decode_failed"
    );
    assert_eq!(
        ReferenceTicketError::InvalidTicket.portable_code(),
        "invalid_ticket"
    );
}
