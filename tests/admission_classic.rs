use lopdf::{Admission, Document, LoadOptions};

fn admission(input: &[u8]) -> lopdf::Result<Admission> {
    Admission::load(input, 1024 * 1024)
}

// Generate exact classic subsections with real offsets, including overlapping ranges.
fn classic(subsections: impl FnOnce(usize) -> String) -> Vec<u8> {
    let mut pdf = b"%PDF-1.5\n1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec();
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!(
            "xref\n{}trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            subsections(9)
        )
        .as_bytes(),
    );
    pdf
}

fn entry(offset: usize, normal: bool) -> String {
    format!("{offset:010} 00000 {} \n", if normal { 'n' } else { 'f' })
}

#[test]
fn repeated_identities_refuse_before_overwrite_in_admission_only() {
    let cases = [
        classic(|o| format!("1 1\n{}1 1\n{}", entry(o, true), entry(o, true))),
        classic(|o| format!("1 1\n{}1 1\n{}", entry(0, false), entry(o, true))),
        classic(|o| format!("1 1\n{}1 1\n{}", entry(o, true), entry(0, false))),
        classic(|o| {
            format!(
                "0 2\n{}{}1 2\n{}{}",
                entry(0, false),
                entry(o, true),
                entry(o, true),
                entry(0, false)
            )
        }),
    ];
    for (i, input) in cases.iter().enumerate() {
        let error = admission(input).err().expect("repeated identity must refuse");
        let section = input.windows(5).position(|w| w == b"xref\n").unwrap();
        assert!(
            matches!(error, lopdf::Error::InvalidStream(ref message) if message == &format!("admission repeated classic xref identity 1 at section offset {section}")),
            "case {i}: {error:?}"
        );
        // Strict and default ordinary readers still apply the old last-entry policy.
        let strict = Document::load_mem_with_options(
            input,
            LoadOptions {
                strict: true,
                ..Default::default()
            },
        );
        assert!(strict.is_ok(), "ordinary strict case {i}: {strict:?}");
        assert!(Document::load_mem(input).is_ok(), "ordinary case {i}");
    }
}

#[test]
fn independent_original_conflicts_refuse() {
    for input in [
        include_bytes!("fixtures/admission_classic/conflicting-table-ambiguous-then-clean.pdf").as_slice(),
        include_bytes!("fixtures/admission_classic/conflicting-table-clean-then-ambiguous.pdf").as_slice(),
        include_bytes!("fixtures/admission_classic/conflicting-table-free-normal.pdf").as_slice(),
    ] {
        assert!(admission(input).is_err());
    }
}

#[test]
fn disjoint_subsections_and_cross_section_replacement_survive() {
    let input = classic(|o| {
        format!(
            "0 1\n{}1 1\n{}3 1\n{}",
            entry(0, false),
            entry(o, true),
            entry(0, false)
        )
    });
    let loaded = admission(&input).unwrap();
    assert_eq!(loaded.sections().len(), 1);
    assert_eq!(loaded.sections()[0].entries().len(), 3);
    assert_eq!(
        loaded.sections()[0].offset(),
        input.windows(5).position(|w| w == b"xref\n").unwrap()
    );
    for input in [
        include_bytes!("fixtures/admission_classic/clean-over-ambiguous.pdf").as_slice(),
        include_bytes!("fixtures/admission_classic/mask-clean-f.pdf").as_slice(),
        include_bytes!("fixtures/admission_classic/hybrid-primary-normal.pdf").as_slice(),
    ] {
        let loaded = admission(input).unwrap();
        assert!(loaded.sections().len() > 1);
        assert_eq!(loaded.source(), input);
    }
}

#[test]
fn malformed_declared_ranges_refuse_admission() {
    for input in [
        classic(|o| format!("1 2\n{}", entry(o, true))),
        classic(|o| {
            format!(
                "1 1\n{}4294967295 2\n{}{}",
                entry(o, true),
                entry(0, false),
                entry(0, false)
            )
        }),
    ] {
        assert!(admission(&input).is_err());
    }
}
