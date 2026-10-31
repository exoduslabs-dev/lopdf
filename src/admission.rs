//! Experimental source-bound admission evidence. Unsupported decoder forms fail closed.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::xref::{Xref, XrefEntry, XrefType};
use crate::{DecompressError, Dictionary, Document, Error, Object, ObjectId, Result, Stream};

#[derive(Debug, Clone, Copy)]
pub enum Relation {
    Primary,
    Prior,
    Supplement,
}

#[derive(Debug, Clone)]
pub struct Section {
    offset: usize,
    parent: Option<usize>,
    relation: Relation,
    kind: XrefType,
    id: Option<ObjectId>,
    entries: BTreeMap<u32, XrefEntry>,
    dictionary: Arc<Dictionary>,
}
impl Section {
    pub fn offset(&self) -> usize {
        self.offset
    }
    pub fn parent(&self) -> Option<usize> {
        self.parent
    }
    pub fn relation(&self) -> Relation {
        self.relation
    }
    pub fn kind(&self) -> XrefType {
        self.kind
    }
    pub fn id(&self) -> Option<ObjectId> {
        self.id
    }
    pub fn entries(&self) -> &BTreeMap<u32, XrefEntry> {
        &self.entries
    }
    pub fn dictionary(&self) -> &Dictionary {
        &self.dictionary
    }
}
#[derive(Debug, Clone)]
pub struct Member {
    id: ObjectId,
    container: ObjectId,
    index: u16,
    dictionary: Arc<Dictionary>,
}
impl Member {
    pub fn id(&self) -> ObjectId {
        self.id
    }
    pub fn container(&self) -> ObjectId {
        self.container
    }
    pub fn index(&self) -> u16 {
        self.index
    }
    pub fn dictionary(&self) -> &Dictionary {
        &self.dictionary
    }
}
/// Owns the exact input and an immutably exposed document and admission evidence.
pub struct Admission {
    source: Vec<u8>,
    document: Document,
    sections: Vec<Section>,
    members: Vec<Member>,
}
impl Admission {
    /// Strict experimental admission. Only unencrypted streams with no filter or a
    /// direct FlateDecode filter and no predictor other than 1 are supported.
    pub fn load(source: &[u8], max_decompressed_size: usize) -> Result<Self> {
        if source.len() > 256 * 1024 * 1024 || !source.starts_with(b"%PDF-") {
            return Err(Error::Unimplemented("admission source bounds or header prefix"));
        }
        let source = source.to_vec();
        let context = Arc::new(Mutex::new(Context::default()));
        let document = crate::reader::read_admission(&source, max_decompressed_size, context.clone())?;
        let mut members = Vec::new();
        let mut containers: BTreeMap<ObjectId, Arc<Dictionary>> = BTreeMap::new();
        for (&number, entry) in &document.reference_table.entries {
            if let XrefEntry::Compressed { container, index } = entry {
                let stream = document
                    .objects
                    .get(&(*container, 0))
                    .ok_or(Error::Unimplemented("missing selected container"))?
                    .as_stream()?;
                let container_id = (*container, 0);
                if !containers.contains_key(&container_id) {
                    let mut ctx = context.lock().unwrap();
                    ctx.charge(dictionary_cost(&stream.dict)?)?;
                    containers.insert(container_id, Arc::new(stream.dict.clone()));
                }
                context.lock().unwrap().charge(128)?;
                members.push(Member {
                    id: (number, 0),
                    container: container_id,
                    index: *index,
                    dictionary: containers[&container_id].clone(),
                });
            }
        }
        let sections = std::mem::take(&mut context.lock().unwrap().sections);
        Ok(Self {
            source,
            document,
            sections,
            members,
        })
    }
    pub fn source(&self) -> &[u8] {
        &self.source
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    /// In precedence order: primary, supplement if present, then each prior revision.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }
    pub fn members(&self) -> &[Member] {
        &self.members
    }
}
#[derive(Default)]
pub(crate) struct Context {
    originals: BTreeMap<usize, (ObjectId, Arc<Dictionary>)>,
    used: usize,
    sections: Vec<Section>,
}
impl Context {
    fn charge(&mut self, amount: usize) -> Result<()> {
        self.used = self
            .used
            .checked_add(amount)
            .filter(|&n| n <= 16 * 1024 * 1024)
            .ok_or(unsupported())?;
        Ok(())
    }
    pub(crate) fn record_dictionary(&mut self, offset: usize, id: ObjectId, dict: &Dictionary) -> Result<()> {
        self.charge(dictionary_cost(dict)?)?;
        self.originals.insert(offset, (id, Arc::new(dict.clone())));
        Ok(())
    }
    pub(crate) fn section(
        &mut self, offset: usize, parent: Option<usize>, relation: Relation, xref: &Xref, trailer: &Dictionary,
    ) -> Result<()> {
        self.charge(
            xref.entries
                .len()
                .checked_mul(128)
                .and_then(|n| n.checked_add(256))
                .ok_or_else(unsupported)?,
        )?;
        let (id, dictionary) = match xref.cross_reference_type {
            XrefType::CrossReferenceStream => {
                let (id, dict) = self
                    .originals
                    .remove(&offset)
                    .ok_or(Error::Unimplemented("missing xref dictionary provenance"))?;
                (Some(id), dict)
            }
            XrefType::CrossReferenceTable => {
                self.charge(dictionary_cost(trailer)?)?;
                (None, Arc::new(trailer.clone()))
            }
        };
        if dictionary.has_duplicate_source_keys() {
            return Err(Error::Unimplemented("duplicate selected xref dictionary"));
        }
        self.sections.push(Section {
            offset,
            parent,
            relation,
            kind: xref.cross_reference_type,
            id,
            entries: xref.entries.clone(),
            dictionary,
        });
        Ok(())
    }
}
fn unsupported() -> Error {
    Error::Unimplemented("unsupported admission decoder provenance")
}
pub(crate) fn check_decoder(stream: &Stream) -> Result<()> {
    let dict = &stream.dict;
    if !matches!(dict.get(b"Length"), Ok(Object::Integer(n)) if usize::try_from(*n).ok() == Some(stream.content.len()))
    {
        return Err(unsupported());
    }
    if dict.len() > 1024
        || dict.iter().any(|(k, _)| k.len() > 4096)
        || dict.has_duplicate_source_keys()
        || dict.has(b"DP")
    {
        return Err(unsupported());
    }
    let filter = dict.get(b"Filter").ok();
    let params = dict.get(b"DecodeParms").ok();
    match filter {
        None if params.is_none() => Ok(()),
        Some(Object::Name(name)) if name == b"FlateDecode" => match params {
            None | Some(Object::Null) => Ok(()),
            Some(Object::Dictionary(p)) if p.len() <= 1024 && !p.has_duplicate_source_keys() => {
                if p.iter().any(|(key, _)| key.as_slice() != b"Predictor") {
                    return Err(unsupported());
                }
                match p.get(b"Predictor") {
                    Err(_) | Ok(Object::Integer(1)) => Ok(()),
                    _ => Err(unsupported()),
                }
            }
            _ => Err(unsupported()),
        },
        _ => Err(unsupported()),
    }
}
// Admission accepts only a complete zlib stream, including its checksum, with
// no unconsumed encoded bytes. The ordinary decoder deliberately remains lenient.
pub(crate) fn decode_content(stream: &Stream, limit: Option<usize>) -> Result<Vec<u8>> {
    check_decoder(stream)?;
    let limit = limit.ok_or_else(unsupported)?;
    if !stream.dict.has(b"Filter") {
        if stream.content.len() > limit {
            return Err(DecompressError::MemoryLimitExceeded { limit }.into());
        }
        return Ok(stream.content.clone());
    }
    let mut decoder = flate2::Decompress::new(true);
    let mut output = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let input_at = usize::try_from(before_in).map_err(|_| unsupported())?;
        let status = decoder
            .decompress(&stream.content[input_at..], &mut chunk, flate2::FlushDecompress::None)
            .map_err(|e| Error::InvalidStream(format!("admission Flate decode failed: {e}")))?;
        let produced = usize::try_from(decoder.total_out() - before_out).map_err(|_| unsupported())?;
        if produced > limit.saturating_sub(output.len()) {
            return Err(DecompressError::MemoryLimitExceeded { limit }.into());
        }
        output.extend_from_slice(&chunk[..produced]);
        if status == flate2::Status::StreamEnd {
            if decoder.total_in() != stream.content.len() as u64 {
                return Err(Error::InvalidStream("trailing admission Flate bytes".into()));
            }
            return Ok(output);
        }
        if decoder.total_in() == before_in && decoder.total_out() == before_out {
            return Err(Error::InvalidStream("incomplete admission Flate stream".into()));
        }
    }
}
pub(crate) fn decode_xref(
    reader: &crate::Reader<'_>, input: &[u8], id: ObjectId, mut stream: Stream,
) -> Result<(Xref, Dictionary)> {
    if let Some(context) = &reader.admission {
        check_decoder(&stream)?;
        let offset = reader
            .buffer
            .len()
            .checked_sub(input.len())
            .ok_or(Error::InvalidOffset(input.len()))?;
        context.lock().unwrap().record_dictionary(offset, id, &stream.dict)?;
        let decoded = decode_content(&stream, reader.max_decompressed_size)?;
        stream.dict.remove(b"Filter");
        stream.dict.remove(b"DecodeParms");
        stream.set_content(decoded);
    }
    crate::parser_aux::decode_xref_stream_with_limit(stream, reader.max_decompressed_size)
}

fn dictionary_cost(dict: &Dictionary) -> Result<usize> {
    fn add(a: usize, b: usize) -> Result<usize> {
        a.checked_add(b).ok_or_else(unsupported)
    }
    fn value_cost(value: &Object, depth: usize) -> Result<usize> {
        if depth > 100 {
            return Err(unsupported());
        }
        let extra = match value {
            Object::Name(v) | Object::String(v, _) => v.len(),
            Object::Array(a) => {
                let mut n = 0usize;
                for item in a {
                    n = add(n, value_cost(item, depth + 1)?)?;
                }
                n
            }
            Object::Dictionary(d) => dict_cost(d, depth + 1)?,
            Object::Stream(s) => add(dict_cost(&s.dict, depth + 1)?, s.content.len())?,
            _ => 0,
        };
        add(128, extra)
    }
    fn dict_cost(dict: &Dictionary, depth: usize) -> Result<usize> {
        if depth > 100 {
            return Err(unsupported());
        }
        let mut n = 128usize;
        for (key, value) in dict.iter() {
            n = add(n, add(key.len(), value_cost(value, depth + 1)?)?)?;
        }
        Ok(n)
    }
    dict_cost(dict, 0)
}

#[cfg(test)]
mod decoder_tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn encoded(data: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }
    fn stream(data: Vec<u8>) -> Stream {
        let mut dict = Dictionary::new();
        dict.set("Filter", "FlateDecode");
        Stream::new(dict, data)
    }
    #[test]
    fn strict_flate_completion_and_limits() {
        for size in [0, 1, 8191, 8192, 8193, 32769] {
            let data: Vec<_> = (0..size).map(|n| (n % 251) as u8).collect();
            let compressed = encoded(&data);
            assert_eq!(decode_content(&stream(compressed.clone()), Some(size)).unwrap(), data);
            if size > 0 {
                assert!(matches!(
                    decode_content(&stream(compressed.clone()), Some(size - 1)),
                    Err(Error::Decompress(DecompressError::MemoryLimitExceeded { .. }))
                ));
            }
            for end in 0..compressed.len() {
                assert!(
                    decode_content(&stream(compressed[..end].to_vec()), Some(size)).is_err(),
                    "truncation {end}/{size}"
                );
            }
            let mut bad = compressed.clone();
            *bad.last_mut().unwrap() ^= 1;
            assert!(decode_content(&stream(bad), Some(size)).is_err());
            let mut tail = compressed;
            tail.extend_from_slice(b"junk");
            assert!(decode_content(&stream(tail), Some(size)).is_err());
        }
        let expanded = vec![42; 100_000];
        assert!(matches!(
            decode_content(&stream(encoded(&expanded)), Some(99999)),
            Err(Error::Decompress(DecompressError::MemoryLimitExceeded { .. }))
        ));
        let raw = Stream::new(Dictionary::new(), b"raw".to_vec());
        assert_eq!(decode_content(&raw, Some(3)).unwrap(), b"raw");
        assert!(decode_content(&raw, Some(2)).is_err());
    }
}
