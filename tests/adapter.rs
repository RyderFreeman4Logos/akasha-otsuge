use akasha_otsuge::adapter::*;
use serde_json::json;
use std::io::Cursor;

fn line(request: &str, sequence: u64, version: u32) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&json!({
        "schema_version":version,"request_id":request,"sequence":sequence,
        "payload":{"event":"started"}
    }))
    .unwrap();
    bytes.push(b'\n');
    bytes
}

#[test]
fn hint_is_optional_unicode_bounded_and_cannot_carry_binding_fields() {
    assert_eq!(ConsultArgs::parse("{}").unwrap().hint, None);
    assert_eq!(
        ConsultArgs::parse(r#"{"hint":"  \t "}"#).unwrap().hint,
        None
    );
    let hint = "字".repeat(512);
    assert_eq!(
        ConsultArgs::parse(&json!({"hint":hint}).to_string())
            .unwrap()
            .hint,
        Some(hint)
    );
    assert_eq!(
        ConsultArgs::parse(&json!({"hint":"字".repeat(513)}).to_string())
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    let literal = "\"$(touch /never)\n`literal`";
    assert_eq!(
        ConsultArgs::parse(&json!({"hint":literal}).to_string())
            .unwrap()
            .hint
            .as_deref(),
        Some(literal)
    );
    for field in [
        "context",
        "session_id",
        "thread_id",
        "cwd",
        "transcript_path",
        "scope",
    ] {
        assert!(ConsultArgs::parse(&json!({field:"forged"}).to_string()).is_err());
    }
}

#[test]
fn jsonl_requires_matching_request_contiguous_sequence_version_and_lf() {
    let mut stream = FrameStream::new("request").unwrap();
    assert_eq!(
        stream
            .read::<serde_json::Value>(&mut Cursor::new(line("request", 0, 1)))
            .unwrap()
            .unwrap()
            .sequence,
        0
    );
    for bad in [
        line("request", 0, 1),
        line("other", 1, 1),
        line("request", 2, 1),
        line("request", 1, 2),
    ] {
        let mut stream = FrameStream::new("request").unwrap();
        stream
            .read::<serde_json::Value>(&mut Cursor::new(line("request", 0, 1)))
            .unwrap();
        assert_eq!(
            stream
                .read::<serde_json::Value>(&mut Cursor::new(bad))
                .unwrap_err()
                .code,
            ErrorCode::ProtocolInvalid
        );
        assert!(
            stream
                .read::<serde_json::Value>(&mut Cursor::new(line("request", 1, 1)))
                .is_err(),
            "failed framing must poison stream"
        );
    }
    let mut partial = line("request", 0, 1);
    partial.pop();
    assert!(
        FrameStream::new("request")
            .unwrap()
            .read::<serde_json::Value>(&mut Cursor::new(partial))
            .is_err()
    );
    assert!(FrameStream::new("").is_err());
}

#[test]
fn jsonl_stops_reading_at_frame_limit_without_allocating_the_input() {
    let mut input = Cursor::new(vec![b'x'; MAX_FRAME_BYTES * 2]);
    let error = FrameStream::new("request")
        .unwrap()
        .read::<serde_json::Value>(&mut input)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::OutputLimit);
    assert!(input.position() <= (MAX_FRAME_BYTES + 1) as u64);
}
