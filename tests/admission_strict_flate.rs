use lopdf::Admission;

#[test]
fn sealed_incomplete_streams_cannot_establish_authority() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/admission_strict_flate/");
    let load = |name: &str| Admission::load(&std::fs::read(format!("{base}{name}.pdf")).unwrap(), 1024 * 1024);
    let full = load("flate-full-number").unwrap();
    assert_eq!(
        full.document().get_object((3, 0)).unwrap().as_i64().unwrap(),
        1234567890
    );
    for name in ["prior-clean", "supplemental-clean"] {
        assert!(load(name).is_ok(), "{name}");
    }
    for name in [
        "prior-checksum",
        "prior-truncated",
        "supplemental-checksum",
        "supplemental-truncated",
        "flate-partial-number",
        "flate-bad-checksum",
        "flate-trailing-junk",
        "flate-truncated-objstm-1",
        "flate-truncated-objstm-10",
        "flate-truncated-objstm-11",
        "xref-flate-bad-checksum",
        "xref-flate-truncated",
    ] {
        assert!(load(name).is_err(), "{name}");
    }
}
