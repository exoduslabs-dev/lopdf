use lopdf::Admission;

fn load(bytes: &[u8]) -> lopdf::Result<Admission> {
    Admission::load(bytes, 1024 * 1024)
}

#[test]
fn object_stream_selection_and_decoder_evidence() {
    let clean = load(include_bytes!("fixtures/admission/clean.pdf")).unwrap();
    assert!(!clean.sections().is_empty());
    assert!(!clean.members().is_empty());
    for member in clean.members() {
        assert_eq!(member.id().1, 0);
        assert_eq!(member.container().1, 0);
        assert!(member.dictionary().has_type(b"ObjStm"));
        assert!(clean.document().objects.contains_key(&member.id()));
    }
    for input in [
        include_bytes!("fixtures/admission/duplicate.pdf").as_slice(),
        include_bytes!("fixtures/admission/escaped.pdf").as_slice(),
        include_bytes!("fixtures/admission/index-mismatch.pdf").as_slice(),
        include_bytes!("fixtures/admission/wrong-generation.pdf").as_slice(),
        include_bytes!("fixtures/admission/dangling-container.pdf").as_slice(),
        include_bytes!("fixtures/admission/parameter-conflict.pdf").as_slice(),
        include_bytes!("fixtures/admission/unsupported-carrier.pdf").as_slice(),
    ] {
        assert!(load(input).is_err());
    }
    assert!(load(include_bytes!("fixtures/admission/inactive-duplicate.pdf")).is_ok());
    assert!(load(include_bytes!("fixtures/admission/free-masked.pdf")).is_ok());
}

#[test]
fn xref_original_decoder_and_hybrid_evidence() {
    for input in [
        include_bytes!("fixtures/admission/xref-clean.pdf").as_slice(),
        include_bytes!("fixtures/admission/xref-supplement-clean.pdf").as_slice(),
    ] {
        let admission = load(input).unwrap();
        assert!(admission.sections().iter().any(|s| s.id().is_some()));
        assert_eq!(admission.source(), input);
    }
    for input in [
        include_bytes!("fixtures/admission/xref-duplicate.pdf").as_slice(),
        include_bytes!("fixtures/admission/xref-escaped.pdf").as_slice(),
        include_bytes!("fixtures/admission/xref-supplement-duplicate.pdf").as_slice(),
        include_bytes!("fixtures/admission/xref-supplement-escaped.pdf").as_slice(),
    ] {
        assert!(load(input).is_err());
    }
}
