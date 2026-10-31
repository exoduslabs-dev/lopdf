use crate::parser;
use crate::{Document, Error, Object, ObjectId, Result, Stream};
use std::collections::BTreeMap;
use std::num::TryFromIntError;
use std::str::FromStr;

use log::warn;
#[cfg(feature = "rayon")]
use rayon::prelude::*;

#[derive(Debug)]
pub struct ObjectStream {
    pub objects: BTreeMap<ObjectId, Object>,
    max_objects: usize,
    compression_level: u32,
}

#[derive(Debug, Clone)]
pub struct ObjectStreamBuilder {
    max_objects: usize,
    compression_level: u32,
}

#[derive(Debug, Clone)]
pub struct ObjectStreamConfig {
    pub max_objects_per_stream: usize,
    pub compression_level: u32,
}

impl Default for ObjectStreamConfig {
    fn default() -> Self {
        Self {
            max_objects_per_stream: 100,
            compression_level: 6,
        }
    }
}

impl ObjectStream {
    /// Parse an existing object stream without modifying its encoded content or
    /// filter dictionary.
    ///
    /// This decodes the stream without any size limit. For untrusted input,
    /// prefer [`ObjectStream::new_with_limit`] to guard against decompression
    /// bombs.
    pub fn new(stream: &Stream) -> Result<ObjectStream> {
        Self::new_with_limit(stream, None)
    }

    /// Parse an existing object stream without modifying it, rejecting the
    /// decoded content if it would exceed `max_decompressed_size` bytes. `None`
    /// means no limit (the behavior of [`ObjectStream::new`]).
    pub fn new_with_limit(stream: &Stream, max_decompressed_size: Option<usize>) -> Result<ObjectStream> {
        let objects = Self::parse_indexed_with_limit(stream, max_decompressed_size)?
            .into_values()
            .collect();
        Ok(ObjectStream {
            objects,
            max_objects: 100,
            compression_level: 6,
        })
    }

    /// Keep header ordinals, including gaps from unreadable members and duplicate IDs.
    pub(crate) fn parse_indexed_with_limit(
        stream: &Stream, max_decompressed_size: Option<usize>,
    ) -> Result<BTreeMap<usize, (ObjectId, Object)>> {
        Self::parse_indexed_inner(stream, max_decompressed_size, false)
    }

    pub(crate) fn parse_indexed_strict(
        stream: &Stream, max_decompressed_size: Option<usize>,
    ) -> Result<BTreeMap<usize, (ObjectId, Object)>> {
        Self::parse_indexed_inner(stream, max_decompressed_size, true)
    }

    fn parse_indexed_inner(
        stream: &Stream, max_decompressed_size: Option<usize>, strict: bool,
    ) -> Result<BTreeMap<usize, (ObjectId, Object)>> {
        let content = if strict {
            crate::admission::decode_content(stream, max_decompressed_size)?
        } else {
            match max_decompressed_size {
                // Object streams are decoded while the document is loaded, so
                // enforcing the limit here bounds the memory a single stream can use.
                Some(max) => stream.get_plain_content_with_limit(max)?,
                None => stream.get_plain_content()?,
            }
        };

        if content.is_empty() {
            return if strict {
                Err(Error::InvalidObjectStream("empty selected object stream".into()))
            } else {
                Ok(BTreeMap::new())
            };
        }

        let first_offset = stream
            .dict
            .get(b"First")
            .and_then(Object::as_i64)?
            .try_into()
            .map_err(|e: TryFromIntError| Error::NumericCast(e.to_string()))?;
        let index_block = content.get(..first_offset).ok_or(Error::InvalidOffset(first_offset))?;

        let numbers_str = std::str::from_utf8(index_block).map_err(|e| Error::InvalidObjectStream(e.to_string()))?;
        let numbers: Vec<_> = numbers_str
            .split_whitespace()
            .map(|number| u32::from_str(number).ok())
            .collect();
        let len = numbers.len() / 2 * 2; // Ensure only pairs.

        let n = stream.dict.get(b"N").and_then(Object::as_i64)?;
        if numbers.len().try_into().ok() != n.checked_mul(2) {
            warn!("object stream: the object stream dictionary specifies a wrong number of objects")
        }

        if strict {
            let expected = usize::try_from(n).ok().and_then(|n| n.checked_mul(2));
            if expected != Some(numbers.len()) || numbers.iter().any(Option::is_none) {
                return Err(Error::InvalidObjectStream(
                    "invalid selected object stream header".into(),
                ));
            }
            let mut seen = std::collections::HashSet::new();
            let mut previous = None;
            for pair in numbers.chunks_exact(2) {
                let id = pair[0].ok_or_else(|| Error::InvalidObjectStream("invalid member id".into()))?;
                let relative =
                    pair[1].ok_or_else(|| Error::InvalidObjectStream("invalid member offset".into()))? as usize;
                let start = first_offset
                    .checked_add(relative)
                    .filter(|&v| v < content.len())
                    .ok_or(Error::InvalidOffset(relative))?;
                if !seen.insert(id) || previous.is_some_and(|p| start <= p) {
                    return Err(Error::InvalidObjectStream(
                        "duplicate or unordered member header".into(),
                    ));
                }
                previous = Some(start);
            }
        }
        let chunks_filter_map = |(index, chunk): (usize, &[_])| {
            let id = chunk[0]?;
            let offset = first_offset.checked_add(chunk[1]? as usize)?;

            if offset >= content.len() {
                warn!("out-of-bounds offset in object stream");
                return None;
            }
            // Skip leading whitespace — some PDFs emit newlines before objects in ObjStm
            let mut start = offset;
            while start < content.len() && content[start].is_ascii_whitespace() {
                start += 1;
            }
            if start >= content.len() {
                warn!("only whitespace after offset in object stream");
                return None;
            }
            let object = if strict {
                let end = numbers
                    .get((index + 1) * 2 + 1)
                    .and_then(|n| *n)
                    .and_then(|relative| first_offset.checked_add(relative as usize))
                    .unwrap_or(content.len());
                parser::direct_object_complete(content.get(start..end)?)?
            } else {
                parser::direct_object(&content[start..])?
            };

            Some((index, ((id, 0), object)))
        };
        #[cfg(feature = "rayon")]
        let objects: BTreeMap<usize, (ObjectId, Object)> = numbers[..len]
            .par_chunks(2)
            .enumerate()
            .filter_map(chunks_filter_map)
            .collect();
        #[cfg(not(feature = "rayon"))]
        let objects: BTreeMap<usize, (ObjectId, Object)> = numbers[..len]
            .chunks(2)
            .enumerate()
            .filter_map(chunks_filter_map)
            .collect();

        if strict && objects.len() != numbers.len() / 2 {
            return Err(Error::InvalidObjectStream(
                "invalid or overlapping selected member".into(),
            ));
        }
        Ok(objects)
    }

    /// Create a builder for constructing new object streams
    pub fn builder() -> ObjectStreamBuilder {
        ObjectStreamBuilder {
            max_objects: 100,
            compression_level: 6,
        }
    }

    /// Add an object to the stream
    pub fn add_object(&mut self, id: ObjectId, obj: Object) -> Result<()> {
        // Check if object can be added to stream
        if matches!(obj, Object::Stream(_)) {
            return Err(Error::InvalidObjectStream(
                "Stream objects cannot be stored in object streams".into(),
            ));
        }

        // Check capacity
        if self.objects.len() >= self.max_objects {
            return Err(Error::InvalidObjectStream(format!(
                "Object stream has reached maximum capacity of {} objects",
                self.max_objects
            )));
        }

        self.objects.insert(id, obj);
        Ok(())
    }

    /// Get the number of objects in the stream
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Build the stream content in the format required by PDF spec
    pub fn build_stream_content(&self) -> Result<Vec<u8>> {
        if self.objects.is_empty() {
            return Ok(Vec::new());
        }

        // Sort objects by ID for consistent output
        let mut sorted_objects: Vec<_> = self.objects.iter().collect();
        sorted_objects.sort_by_key(|(id, _)| *id);

        // First build the offset table to know its size
        let mut offset_entries = Vec::new();
        let mut current_offset = 0;

        for ((obj_num, _gen), obj) in &sorted_objects {
            // Store the object number and its offset
            offset_entries.push(format!("{obj_num} {current_offset}"));

            // Calculate size of this object's serialization
            let mut obj_bytes = Vec::new();
            crate::writer::Writer::write_object(&mut obj_bytes, obj)?;
            current_offset += obj_bytes.len() + 1; // +1 for space separator
        }

        // Build the complete offset table with proper spacing
        let offset_table = offset_entries.join(" ") + " ";

        // Now build the final content
        let mut content = Vec::new();
        content.extend_from_slice(offset_table.as_bytes());

        // Add serialized objects with space separators
        for ((_, _), obj) in &sorted_objects {
            let mut obj_bytes = Vec::new();
            crate::writer::Writer::write_object(&mut obj_bytes, obj)?;
            content.extend_from_slice(&obj_bytes);
            content.push(b' '); // Space separator between objects
        }

        Ok(content)
    }

    /// Convert to a Stream object ready for insertion into a PDF
    pub fn to_stream_object(&self) -> Result<Stream> {
        let content = self.build_stream_content()?;

        // Calculate where the first object starts
        // We need to find the size of the offset table
        let mut sorted_objects: Vec<_> = self.objects.iter().collect();
        sorted_objects.sort_by_key(|(id, _)| *id);

        // Build the offset entries to calculate exact size
        let mut offset_entries = Vec::new();
        let mut current_offset = 0;

        for ((obj_num, _gen), obj) in &sorted_objects {
            offset_entries.push(format!("{obj_num} {current_offset}"));

            // Calculate size of this object's serialization
            let mut obj_bytes = Vec::new();
            crate::writer::Writer::write_object(&mut obj_bytes, obj)?;
            current_offset += obj_bytes.len() + 1; // +1 for space separator
        }

        // The offset table is joined with spaces and has a trailing space
        let offset_table = offset_entries.join(" ") + " ";
        let first_offset = offset_table.len();

        let dict = dictionary! {
            "Type" => "ObjStm",
            "N" => self.objects.len() as i64,
            "First" => first_offset as i64,
        };

        let mut stream = Stream::new(dict, content);

        // Apply compression - object streams should always be compressed
        if self.compression_level > 0 {
            // Force compression by setting Filter directly
            use flate2::Compression;
            use flate2::write::ZlibEncoder;
            use std::io::prelude::*;

            let compression = match self.compression_level {
                0 => Compression::none(),
                1..=3 => Compression::fast(),
                4..=6 => Compression::default(),
                _ => Compression::best(),
            };

            let mut encoder = ZlibEncoder::new(Vec::new(), compression);
            encoder.write_all(&stream.content)?;
            let compressed = encoder.finish()?;

            stream.dict.set("Filter", "FlateDecode");
            stream.set_content(compressed);
        }

        Ok(stream)
    }

    /// Check if an object can be compressed into an object stream
    pub fn can_be_compressed(id: ObjectId, obj: &Object, doc: &Document) -> bool {
        // Rule 1: Stream objects cannot be compressed
        if matches!(obj, Object::Stream(_)) {
            return false;
        }

        // Rule 2: Objects with non-zero generation cannot be compressed
        if id.1 != 0 {
            return false;
        }

        // Rule 3: Only encryption dictionary cannot be compressed from trailer references
        if let Ok(Object::Reference(encrypt_ref)) = doc.trailer.get(b"Encrypt")
            && id == *encrypt_ref
        {
            return false;
        }

        // Rule 4: Specific object types that cannot be compressed
        if let Object::Dictionary(dict) = obj
            && let Ok(type_obj) = dict.get(b"Type")
            && let Ok(type_name) = type_obj.as_name()
        {
            match type_name {
                // Cross-reference streams and object streams cannot be compressed
                b"XRef" => return false,
                b"ObjStm" => return false,

                // Catalog can only be excluded in linearized PDFs
                b"Catalog" if Self::is_linearized(doc) => {
                    return false;
                }
                b"Catalog" => {}

                // Page, Pages, and all other types CAN be compressed
                _ => {}
            }
        }

        // Default: Allow compression
        true
    }

    /// Check if a PDF document is linearized
    fn is_linearized(doc: &Document) -> bool {
        // In a linearized PDF, the first object after the header should be a
        // linearization dictionary with /Linearized entry
        // For simplicity, we check if any object has a /Linearized entry
        for obj in doc.objects.values() {
            if let Object::Dictionary(dict) = obj
                && dict.has(b"Linearized")
            {
                return true;
            }
        }
        false
    }
}

impl ObjectStreamBuilder {
    /// Set the maximum number of objects per stream
    pub fn max_objects(mut self, max: usize) -> Self {
        self.max_objects = max;
        self
    }

    /// Set the compression level (0-9)
    pub fn compression_level(mut self, level: u32) -> Self {
        self.compression_level = level;
        self
    }

    /// Build the ObjectStream
    pub fn build(self) -> ObjectStream {
        ObjectStream {
            objects: BTreeMap::new(),
            max_objects: self.max_objects,
            compression_level: self.compression_level,
        }
    }

    /// Get the current max_objects setting
    pub fn get_max_objects(&self) -> usize {
        self.max_objects
    }

    /// Get the current compression_level setting
    pub fn get_compression_level(&self) -> u32 {
        self.compression_level
    }
}

#[cfg(test)]
mod admission_header_tests {
    use super::*;
    use crate::Dictionary;

    fn selected(content: &[u8], first: i64, count: i64) -> Stream {
        let mut dictionary = Dictionary::new();
        dictionary.set("First", first);
        dictionary.set("N", count);
        Stream::new(dictionary, content.to_vec())
    }

    #[test]
    fn selected_header_rejects_ambiguous_and_overlapping_members() {
        assert!(ObjectStream::parse_indexed_strict(&selected(b"1 0 2 2 7 8", 8, 2), Some(100)).is_ok());
        assert!(ObjectStream::parse_indexed_strict(&selected(b"1 0 1 2 7 8", 8, 2), Some(100)).is_err());
        assert!(ObjectStream::parse_indexed_strict(&selected(b"1 0 2 0 7 8", 8, 2), Some(100)).is_err());
        assert!(ObjectStream::parse_indexed_strict(&selected(b"1 0 2 2 7 8", 8, 1), Some(100)).is_err());
        assert!(ObjectStream::parse_indexed_strict(&selected(b"1 0 2 1 77 8", 8, 2), Some(100)).is_err());
        assert!(ObjectStream::parse_indexed_strict(&selected(b"1 x 7", 4, 1), Some(100)).is_err());
    }
}
