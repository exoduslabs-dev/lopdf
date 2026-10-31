use super::{dict_dup, direct_object};
use crate::{Dictionary, Document, Object};

fn dict(input: &[u8]) -> Dictionary {
    direct_object(input).unwrap().as_dict().unwrap().clone()
}

#[test]
fn decoded_key_matrix_and_winners() {
    for (input, marked, value) in [
        (b"<< /K 1 >>".as_slice(), false, 1),
        (b"<< /K 1 /K 1 >>".as_slice(), true, 1),
        (b"<< /K 1 /K 2 >>".as_slice(), true, 2),
        (b"<< /K 1 /#4b 3 >>".as_slice(), true, 3),
    ] {
        let d = dict(input);
        assert_eq!(d.has_duplicate_source_keys(), marked);
        assert_eq!(d.get(b"K").unwrap().as_i64().unwrap(), value);
    }
}

#[test]
fn legacy_dict_dup_fold_retains_winner_and_marker() {
    let (rest, d) = dict_dup(b"2 dict dup begin /K 1 def /#4b 2 def end").unwrap();
    assert!(rest.is_empty());
    assert!(d.has_duplicate_source_keys());
    assert_eq!(d.get(b"K").unwrap().as_i64().unwrap(), 2);
    assert!(
        !dict_dup(b"1 dict dup begin /K 1 def end")
            .unwrap()
            .1
            .has_duplicate_source_keys()
    );
}

#[test]
fn nested_and_separate_dictionaries_do_not_taint_parent() {
    let d = dict(b"<< /A << /K 1 /K 2 >> /B << /K 2 >> >>");
    assert!(!d.has_duplicate_source_keys());
    assert!(d.get(b"A").unwrap().as_dict().unwrap().has_duplicate_source_keys());
    assert!(!d.get(b"B").unwrap().as_dict().unwrap().has_duplicate_source_keys());
}

#[test]
fn historical_marker_and_reconstruction_limits() {
    let source = dict(b"<< /K 1 /K 2 >>");
    let clean = dict(b"<< /K 2 >>");
    assert_eq!(source, clean);
    assert_eq!(Object::Dictionary(source.clone()), Object::Dictionary(clean));
    let mut changed = source.clone();
    changed.set("K", 3);
    *changed.get_mut(b"K").unwrap() = Object::Null;
    for (_, v) in changed.iter_mut() {
        *v = Object::Integer(4);
    }
    changed.remove(b"K");
    assert!(changed.is_empty());
    assert!(changed.has_duplicate_source_keys());
    let rebuilt: Dictionary = source.clone().into_iter().collect();
    assert_eq!(source, rebuilt);
    assert!(!rebuilt.has_duplicate_source_keys());
    let mut extended = Dictionary::new();
    extended.extend(&source);
    assert_eq!(source, extended);
    assert!(!extended.has_duplicate_source_keys());
    changed.extend(&source);
    assert!(changed.has_duplicate_source_keys());
    let mut generated = Dictionary::new();
    generated.set("K", 1);
    generated.set("K", 2);
    assert_eq!(source, generated);
    assert!(!generated.has_duplicate_source_keys());
}

#[test]
fn malformed_dictionaries_refuse_instead_of_producing_clean_facts() {
    for input in [
        b"<< /K 1 /K >>".as_slice(),
        b"<< /K 1 /K 2".as_slice(),
        b"<< /K 1 /K (unterminated >>".as_slice(),
    ] {
        assert!(direct_object(input).is_none(), "{input:?}");
    }
}

#[test]
fn repeated_key_work_is_bounded_by_input_and_depth_budget_is_retained() {
    // Finite resource regression, not a hostile-input CPU/RSS guarantee.
    let mut input = b"<< ".to_vec();
    for _ in 0..100_000 {
        input.extend_from_slice(b"/K 1 ");
    }
    input.extend_from_slice(b">>");
    let d = dict(&input);
    assert_eq!(d.len(), 1);
    assert!(d.has_duplicate_source_keys());
    assert!(
        std::mem::size_of::<Dictionary>()
            <= std::mem::size_of::<indexmap::IndexMap<Vec<u8>, Object>>() + std::mem::align_of::<Dictionary>()
    );
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let mut deep = Vec::new();
            for _ in 0..200 {
                deep.extend_from_slice(b"<< /K ");
            }
            deep.push(b'0');
            for _ in 0..200 {
                deep.extend_from_slice(b" >>");
            }
            assert!(direct_object(&deep).is_none());
        })
        .unwrap()
        .join()
        .unwrap();
}

const CLEAN: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a3 0 obj\x0a<< /Type /OCG /Name (Dieline) /GTS_Metadata << /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >> >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a5 0 obj\x0a<< /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0axref\x0a0 7\x0a0000000000 65535 f \x0a0000000009 00000 n \x0a0000000116 00000 n \x0a0000000173 00000 n \x0a0000000301 00000 n \x0a0000000430 00000 n \x0a0000000511 00000 n \x0atrailer\x0a<< /Size 7 /Root 1 0 R >>\x0astartxref\x0a598\x0a%%EOF\x0a";

const ESCAPED_LAST_WINS: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a3 0 obj\x0a<< /Type /OCG /Name (Dieline) /GTS_Metadata 99 0 R /GTS#5fMetadata << /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >> >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a5 0 obj\x0a<< /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0axref\x0a0 7\x0a0000000000 65535 f \x0a0000000009 00000 n \x0a0000000116 00000 n \x0a0000000173 00000 n \x0a0000000324 00000 n \x0a0000000453 00000 n \x0a0000000534 00000 n \x0atrailer\x0a<< /Size 7 /Root 1 0 R >>\x0astartxref\x0a621\x0a%%EOF\x0a";

const ACTIVE_CLEAN: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a3 0 obj\x0a<< /Type /OCG /Name (Dieline) /GTS_Metadata 99 0 R /GTS_Metadata << /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >> >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a5 0 obj\x0a<< /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0axref\x0a0 7\x0a0000000000 65535 f \x0a0000000009 00000 n \x0a0000000116 00000 n \x0a0000000173 00000 n \x0a0000000322 00000 n \x0a0000000451 00000 n \x0a0000000532 00000 n \x0atrailer\x0a<< /Size 7 /Root 1 0 R >>\x0astartxref\x0a619\x0a%%EOF\x0a3 0 obj\x0a<< /Type /OCG /Name (Dieline) /GTS_Metadata << /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >> >>\x0aendobj\x0axref\x0a3 1\x0a0000000822 00000 n \x0atrailer\x0a<< /Size 7 /Root 1 0 R /Prev 619 >>\x0astartxref\x0a950\x0a%%EOF\x0a";

const ACTIVE_DUPLICATE: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a3 0 obj\x0a<< /Type /OCG /Name (Dieline) /GTS_Metadata << /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >> >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a5 0 obj\x0a<< /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0axref\x0a0 7\x0a0000000000 65535 f \x0a0000000009 00000 n \x0a0000000116 00000 n \x0a0000000173 00000 n \x0a0000000301 00000 n \x0a0000000430 00000 n \x0a0000000511 00000 n \x0atrailer\x0a<< /Size 7 /Root 1 0 R >>\x0astartxref\x0a598\x0a%%EOF\x0a3 0 obj\x0a<< /Type /OCG /Name (Dieline) /GTS_Metadata 99 0 R /GTS_Metadata << /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >> >>\x0aendobj\x0axref\x0a3 1\x0a0000000801 00000 n \x0atrailer\x0a<< /Size 7 /Root 1 0 R /Prev 598 >>\x0astartxref\x0a950\x0a%%EOF\x0a";

const COMPRESSED_OWNER_DUPLICATE: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a5 0 obj\x0a<< /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0a8 0 obj\x0a<< /Length 108 /Type /ObjStm /N 1 /First 4 /Filter /FlateDecode >>\x0astream\x0ax\x9c3V0P\xb0\xb1Q\xd0\x0f\xa9,HU\xd0\xf7wvW\xd0\xf7K\xccMU\xd0p\xc9L\xcd\xc9\xccK\xd5T\xd0w\x0f\x09\x8e\xf7M-ILI,IT\xb0\xb4\x04j\x08B\x13\x04\x19\x00\x12\x08(\xcaO\x0e.I-(v/\xca/-P\xd0\x0f.)*M.)-J\xccA\x93\x87\xd8\xe6\\ZR\x92\x99\x97\xae`g\x07B\x00S\xdd+\xd0\x0aendstream\x0aendobj\x0a9 0 obj\x0a<< /Length 70 /Type /XRef /Size 10 /Root 1 0 R /W [1 4 2] >>\x0astream\x0a\x00\x00\x00\x00\x00\xff\xff\x01\x00\x00\x00\x09\x00\x00\x01\x00\x00\x00t\x00\x00\x02\x00\x00\x00\x08\x00\x00\x01\x00\x00\x00\xad\x00\x00\x01\x00\x00\x01.\x00\x00\x01\x00\x00\x01\x7f\x00\x00\x00\x00\x00\x00\x00\xff\xff\x01\x00\x00\x01\xd6\x00\x00\x01\x00\x00\x02\xa6\x00\x00\x0aendstream\x0aendobj\x0astartxref\x0a678\x0a%%EOF\x0a";

const COMPRESSED_METADATA_DUPLICATE: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0a8 0 obj\x0a<< /Length 77 /Type /ObjStm /N 2 /First 9 /Filter /FlateDecode >>\x0astream\x0ax\x9c3V0P0U05V\xb0\xb1Q\xd0\x0f\xa9,HU\xd0\xf7wvW\xd0\xf7K\xccMU\xd0p\xc9L\xcd\xc9\xccK\xd5T\xd0w\x0f\x09\x8e\xf7M-ILI,I\x04j0P\x08R\xb0\xb3\x03k\x8aP0\x04\x11F >\x00\x1a\xf9\x13\xc2\x0aendstream\x0aendobj\x0a9 0 obj\x0a<< /Length 70 /Type /XRef /Size 10 /Root 1 0 R /W [1 4 2] >>\x0astream\x0a\x00\x00\x00\x00\x00\xff\xff\x01\x00\x00\x00\x09\x00\x00\x01\x00\x00\x00t\x00\x00\x02\x00\x00\x00\x08\x00\x00\x01\x00\x00\x00\xad\x00\x00\x02\x00\x00\x00\x08\x00\x01\x01\x00\x00\x01.\x00\x00\x00\x00\x00\x00\x00\xff\xff\x01\x00\x00\x01\x85\x00\x00\x01\x00\x00\x025\x00\x00\x0aendstream\x0aendobj\x0astartxref\x0a565\x0a%%EOF\x0a";

const COMPRESSED_CLEAN: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0a8 0 obj\x0a<< /Length 109 /Type /ObjStm /N 2 /First 10 /Filter /FlateDecode >>\x0astream\x0ax\x9c3V0P0U044V\xb0\xb1Q\xd0\x0f\xa9,HU\xd0\xf7wvW\xd0\xf7K\xccMU\xd0p\xc9L\xcd\xc9\xccK\xd5T\xd0w\x0f\x09\x8e\xf7M-ILI,I\x04\xab\x05\x09\x04\x14\xe5'\x07\x97\xa4\x16\x14\xbb\x17\xe5\x97\x16(\xe8\x07\x97\x14\x95&\x97\x94\x16%\xe6\xa0\xc9C\x0cv.-)\xc9\xccKW\xb0\xb3\x03!\xca\x0d\x01\x00\xd1\x1b=\xdf\x0aendstream\x0aendobj\x0a9 0 obj\x0a<< /Length 70 /Type /XRef /Size 10 /Root 1 0 R /W [1 4 2] >>\x0astream\x0a\x00\x00\x00\x00\x00\xff\xff\x01\x00\x00\x00\x09\x00\x00\x01\x00\x00\x00t\x00\x00\x02\x00\x00\x00\x08\x00\x00\x01\x00\x00\x00\xad\x00\x00\x02\x00\x00\x00\x08\x00\x01\x01\x00\x00\x01.\x00\x00\x00\x00\x00\x00\x00\xff\xff\x01\x00\x00\x01\x85\x00\x00\x01\x00\x00\x02W\x00\x00\x0aendstream\x0aendobj\x0astartxref\x0a599\x0a%%EOF\x0a";

#[test]
fn loaded_active_revisions_and_filtered_object_stream_members() {
    for (bytes, owner_marked, metadata_marked) in [
        (CLEAN, false, false),
        (ESCAPED_LAST_WINS, true, false),
        (ACTIVE_CLEAN, false, false),
        (ACTIVE_DUPLICATE, true, false),
        (COMPRESSED_OWNER_DUPLICATE, true, false),
        (COMPRESSED_METADATA_DUPLICATE, false, true),
        (COMPRESSED_CLEAN, false, false),
    ] {
        let doc = Document::load_mem(bytes).unwrap();
        let owner = doc.objects[&(3, 0)].as_dict().unwrap();
        assert_eq!(owner.has_duplicate_source_keys(), owner_marked);
        if metadata_marked {
            assert!(doc.objects[&(5, 0)].as_dict().unwrap().has_duplicate_source_keys());
        }
    }
}

#[test]
fn stream_dictionary_and_save_reload_keep_values_but_not_source_authority() {
    let mut doc = Document::load_mem(ESCAPED_LAST_WINS).unwrap();
    assert!(
        !doc.objects[&(6, 0)]
            .as_stream()
            .unwrap()
            .dict
            .has_duplicate_source_keys()
    );
    let original = doc.objects[&(3, 0)].clone();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let reloaded = Document::load_mem(&bytes).unwrap();
    assert_eq!(reloaded.objects[&(3, 0)], original);
    assert!(!reloaded.objects[&(3, 0)].as_dict().unwrap().has_duplicate_source_keys());
}

const UNFILTERED_OBJECT_STREAM: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << >> >>\x0aendobj\x0a5 0 obj\x0a<< /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >>\x0aendobj\x0a6 0 obj\x0a<< /Type /ObjStm /N 1 /First 4 /Length 74 >>\x0astream\x0a3 0 << /Type /OCG /Name (Dieline) /GTS_Metadata null /GTS_Metadata null >>\x0aendstream\x0aendobj\x0a7 0 obj\x0a<< /Type /XRef /Size 8 /Root 1 0 R /W [1 4 2] /Length 56 >>\x0astream\x0a\x00\x00\x00\x00\x00\xff\xff\x01\x00\x00\x00\x09\x00\x00\x01\x00\x00\x00\\\x00\x00\x02\x00\x00\x00\x06\x00\x00\x01\x00\x00\x00\x95\x00\x00\x01\x00\x00\x00\xeb\x00\x00\x01\x00\x00\x01<\x00\x00\x01\x00\x00\x01\xd4\x00\x00\x0aendstream\x0aendobj\x0astartxref\x0a468\x0a%%EOF\x0a";

const DUPLICATE_STREAM: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a3 0 obj\x0a<< /Type /OCG /Name (Dieline) /GTS_Metadata << /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >> >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a5 0 obj\x0a<< /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0a7 0 obj\x0a<< /Length 0 /Private 1 /Private 2 >>\x0astream\x0a\x0aendstream\x0aendobj\x0axref\x0a0 8\x0a0000000000 65535 f \x0a0000000009 00000 n \x0a0000000116 00000 n \x0a0000000173 00000 n \x0a0000000301 00000 n \x0a0000000430 00000 n \x0a0000000511 00000 n \x0a0000000598 00000 n \x0atrailer\x0a<< /Size 8 /Root 1 0 R >>\x0astartxref\x0a669\x0a%%EOF\x0a";

const MALFORMED_OWNER: &[u8] = b"%PDF-1.7\x0a1 0 obj\x0a<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [3 0 R] /D << /Order [3 0 R] >> >> >>\x0aendobj\x0a2 0 obj\x0a<< /Type /Pages /Count 1 /Kids [4 0 R] >>\x0aendobj\x0a3 0 obj\x0a<< /Type /OCG /K 1 /K >>\x0aendobj\x0a4 0 obj\x0a<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Resources << /Properties << /L 3 0 R >> >> /Contents 6 0 R >>\x0aendobj\x0a5 0 obj\x0a<< /GTS_ProcStepsGroup /Structural /GTS_ProcStepsType /Cutting >>\x0aendobj\x0a6 0 obj\x0a<< /Length 36  >>\x0astream\x0a/OC /L BDC 1 1 m 9 1 l 9 9 l h S EMC\x0aendstream\x0aendobj\x0axref\x0a0 7\x0a0000000000 65535 f \x0a0000000009 00000 n \x0a0000000116 00000 n \x0a0000000173 00000 n \x0a0000000213 00000 n \x0a0000000342 00000 n \x0a0000000423 00000 n \x0atrailer\x0a<< /Size 7 /Root 1 0 R >>\x0astartxref\x0a510\x0a%%EOF\x0a";

#[test]
fn unfiltered_object_stream_and_stream_dictionary_duplicates() {
    let doc = Document::load_mem(UNFILTERED_OBJECT_STREAM).unwrap();
    assert!(doc.objects[&(3, 0)].as_dict().unwrap().has_duplicate_source_keys());
    let doc = Document::load_mem(DUPLICATE_STREAM).unwrap();
    assert!(
        doc.objects[&(7, 0)]
            .as_stream()
            .unwrap()
            .dict
            .has_duplicate_source_keys()
    );
    assert!(!doc.objects[&(3, 0)].as_dict().unwrap().has_duplicate_source_keys());
}

#[test]
fn recovery_limits_remain_explicit_not_clean_source_permission() {
    use crate::LoadOptions;
    // Existing lenient loading may omit a malformed selected object.
    let doc = Document::load_mem(MALFORMED_OWNER).unwrap();
    assert!(!doc.objects.contains_key(&(3, 0)));
    assert!(
        Document::load_mem_with_options(
            MALFORMED_OWNER,
            LoadOptions {
                strict: true,
                ..Default::default()
            }
        )
        .is_err()
    );
    // Existing bounded final-xref recovery still materializes duplicate facts.
    let text = std::str::from_utf8(ESCAPED_LAST_WINS).unwrap();
    let (prefix, tail) = text.rsplit_once("startxref\n").unwrap();
    let old: usize = tail.lines().next().unwrap().parse().unwrap();
    let recovered = format!("{prefix}startxref\n{}\n%%EOF\n", old + 4);
    let doc = Document::load_mem(recovered.as_bytes()).unwrap();
    assert!(doc.objects[&(3, 0)].as_dict().unwrap().has_duplicate_source_keys());
    assert!(
        Document::load_mem_with_options(
            recovered.as_bytes(),
            LoadOptions {
                strict: true,
                ..Default::default()
            }
        )
        .is_err()
    );
    // A caller requesting a tiny decode budget must not get a clean member.
    assert!(
        Document::load_mem_with_options(
            COMPRESSED_OWNER_DUPLICATE,
            LoadOptions {
                strict: true,
                max_decompressed_size: Some(8),
                ..Default::default()
            }
        )
        .is_err()
    );
}
