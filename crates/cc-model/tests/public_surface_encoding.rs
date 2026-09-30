//! Hand-authored nonempty wire golden, independent of production encoders.
use cc_model::public_surface::*;
#[test]
fn nonempty_encoding_pins_visibility_tokens_and_forwarding() {
    let mut s = PublicSurface::new("r", "m", "v");
    s.entries.push(SurfaceEntry {
        qualified_name: "a".into(),
        exported_name: "b".into(),
        kind: "f".into(),
        visibility: VisibilityDomain::Restricted("x".into()),
        signature: vec![
            SurfaceToken {
                kind: "id".into(),
                text: "a".into(),
            },
            SurfaceToken {
                kind: "str".into(),
                text: "a b".into(),
            },
        ],
        conditions: vec!["cfg".into()],
    });
    s.forwards.push(SurfaceForward {
        source: "./p".into(),
        imported_name: "*".into(),
        exported_name: "ns".into(),
        visibility: VisibilityDomain::Exported,
        type_only: true,
    });
    s.normalize();
    let mut expected = b"codecortex.public-surface\0".to_vec();
    expected.extend([1, 0, 0, 0]);
    // Every string length and every tag below is fixed by the documented format.
    expected.extend([
        1, 0, 0, 0, 0, 0, 0, 0, b'v', 1, 0, 0, 0, 0, 0, 0, 0, b'r', 1, 0, 0, 0, 0, 0, 0, 0, b'm', 1,
    ]);
    expected.extend([1, 0, 0, 0, 0, 0, 0, 0]); // one entry
    for ch in *b"abf" {
        expected.extend([1, 0, 0, 0, 0, 0, 0, 0, ch]);
    }
    expected.extend([4, 1, 0, 0, 0, 0, 0, 0, 0, b'x']); // restricted visibility
    expected.extend([2, 0, 0, 0, 0, 0, 0, 0]); // two ordered tokens
    expected.extend([
        2, 0, 0, 0, 0, 0, 0, 0, b'i', b'd', 1, 0, 0, 0, 0, 0, 0, 0, b'a',
    ]);
    expected.extend([
        3, 0, 0, 0, 0, 0, 0, 0, b's', b't', b'r', 3, 0, 0, 0, 0, 0, 0, 0, b'a', b' ', b'b',
    ]);
    expected.extend([
        1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, b'c', b'f', b'g',
    ]);
    expected.extend([1, 0, 0, 0, 0, 0, 0, 0]); // one forwarding record
    expected.extend([
        3, 0, 0, 0, 0, 0, 0, 0, b'.', b'/', b'p', 1, 0, 0, 0, 0, 0, 0, 0, b'*', 2, 0, 0, 0, 0, 0,
        0, 0, b'n', b's', 1, 1,
    ]);
    expected.extend([0; 16]); // no global conditions or unknown reasons
    assert_eq!(s.canonical_bytes().unwrap(), expected);
    assert_eq!(
        s.fingerprint().unwrap(),
        format!("ps1:{}", blake3::hash(&expected).to_hex())
    );
    let old = s.fingerprint();
    s.entries[0].signature.reverse();
    assert_ne!(old, s.fingerprint());
    s.entries[0].signature.reverse();
    s.forwards[0].type_only = false;
    assert_ne!(old, s.fingerprint());
}
