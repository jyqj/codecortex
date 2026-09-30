use cc_model::source::*;
#[test]
fn byte_positions_crlf_utf8_eof_and_raw_identity_have_independent_gold() {
    let raw = "甲\r\nβ\n".as_bytes();
    let s = SourceSnapshot::new(raw);
    assert_eq!(raw.len(), 8);
    assert_eq!(s.line_count(), 2);
    assert_eq!(s.point(0).unwrap(), (1, 0));
    assert_eq!(s.point(3).unwrap(), (1, 3));
    assert_eq!(s.point(5).unwrap(), (2, 0));
    assert_eq!(s.point(8).unwrap(), (3, 0));
    assert_eq!(s.line_range(1, 1).unwrap(), ByteSpan { start: 0, end: 5 });
    assert_eq!(s.line_range(2, 2).unwrap(), ByteSpan { start: 5, end: 8 });
    assert_eq!(s.lines(ByteSpan { start: 0, end: 5 }).unwrap(), (1, 1));
    assert_eq!(s.lines(ByteSpan { start: 5, end: 7 }).unwrap(), (2, 2));
    assert_eq!(s.slice(ByteSpan { start: 0, end: 5 }).unwrap(), "甲\r\n");
    assert!(s.slice(ByteSpan { start: 1, end: 5 }).is_err());
    assert!(s.raw_slice(ByteSpan { start: 8, end: 7 }).is_err());
    assert!(s.lines(ByteSpan { start: 8, end: 8 }).is_err());
    assert!(s.point(9).is_err());
    assert_eq!(
        s.identity().content_digest,
        blake3::hash(raw).to_hex().as_str()
    );
    assert_ne!(
        s.identity().snapshot_id,
        SourceSnapshot::new("甲\nβ\n".as_bytes())
            .identity()
            .snapshot_id
    );
}
#[test]
fn opaque_bytes_are_preserved_not_lossily_decoded() {
    let raw = [0xff, 0, 0x0a];
    let s = SourceSnapshot::new(&raw);
    assert_eq!(s.identity().encoding, SourceEncoding::Opaque);
    assert_eq!(s.bytes(), &raw);
    assert!(s.text().is_err());
    assert_eq!(s.raw_slice(s.whole()).unwrap(), &raw);
}
#[test]
fn empty_bom_lone_cr_and_final_lf_are_distinct() {
    for (text, lines) in [
        ("", 0),
        ("\n", 1),
        ("\n\n", 2),
        ("a\r", 1),
        ("a\r\nb", 2),
        ("\u{feff}x\r\n", 1),
    ] {
        let s = SourceSnapshot::new(text.as_bytes());
        assert_eq!(s.line_count(), lines);
        assert_eq!(s.slice(s.whole()).unwrap(), text);
    }
    assert!(ByteSpan::new(2, 1).is_err());
}
