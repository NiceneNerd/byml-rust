use crate::{Byml, Endian, NodeType, U24};
use binwrite::{BinWrite, WriterOption};
use byteorder::{BigEndian, ByteOrder, LittleEndian};
use indexmap::{IndexMap, IndexSet};
use rayon::prelude::*;
use std::collections::{hash_map::DefaultHasher, BTreeMap};
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Seek, SeekFrom, Write};

type WriteResult = Result<(), WriteError>;

#[derive(Debug)]
pub struct WriteError(String);

impl Error for WriteError {}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Error writing BYML: {}", self.0)
    }
}

impl From<std::io::Error> for WriteError {
    fn from(err: std::io::Error) -> WriteError {
        WriteError(format!("{}", err))
    }
}

impl Byml {
    /// Serialize the document to binary data with the specified endianness and version. Only hash,
    /// array, or null nodes can be used.
    pub fn to_binary(&self, endian: Endian, version: u16) -> Result<Vec<u8>, WriteError> {
        let mut buf: Vec<u8> = Vec::new();
        self.write_binary(&mut Cursor::new(&mut buf), endian, version)?;
        Ok(buf)
    }

    /// Serialize the document to binary data with the specified endianness and version and yaz0
    /// compress it. Only hash, array, or null nodes can be used.
    pub fn to_compressed_binary(
        &self,
        endian: Endian,
        version: u16,
    ) -> Result<Vec<u8>, WriteError> {
        let mut buf: Vec<u8> = Vec::new();
        let mut writer = Cursor::new(&mut buf);
        let yaz_writer = yaz0::Yaz0Writer::new(&mut writer);
        match yaz_writer.compress_and_write(
            &self.to_binary(endian, version)?,
            yaz0::CompressionLevel::Lookahead { quality: 10 },
        ) {
            Ok(()) => Ok(buf),
            Err(e) => Err(WriteError(format!("{}", e))),
        }
    }

    /// Write the binary serialized BYML document to a writer with the specified endianness and
    /// version. Only hash, array, or null nodes can be used.
    pub fn write_binary<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        version: u16,
    ) -> WriteResult {
        if version > 4 || version < 2 {
            return Err(WriteError(format!(
                "Version {} unsupported, expected 2-4",
                version
            )));
        }
        match self {
            Byml::Array(_) | Byml::Hash(_) | Byml::Null => {
                let mut byml_writer = BymlWriter::new(writer, self, endian.into(), version);
                byml_writer.write_doc()?;
                Ok(())
            }
            _ => Err(WriteError(format!(
                "Can only serialize array, hash, or null nodes, found {:?}",
                self.get_type()
            ))),
        }
    }
}

#[derive(Debug, BinWrite)]
struct Header {
    magic: [u8; 2],
    version: u16,
    hash_table_offset: u32,
    string_table_offset: u32,
    root_node_offset: u32,
}

#[derive(Debug, BinWrite)]
struct Node {
    r#type: NodeType,
    value: NodeValue,
}

#[derive(Debug, BinWrite)]
struct HashNode {
    count: U24,
    entries: Vec<HashEntry>,
}

#[derive(Debug, BinWrite)]
struct ArrayNode {
    count: U24,
    types: Vec<NodeType>,
}

#[derive(Debug, BinWrite)]
struct HashEntry {
    key_idx: U24,
    r#type: NodeType,
    value: NodeValue,
}

#[derive(Debug, PartialEq)]
enum NodeValue {
    Bool(bool),
    Int(i32),
    UInt(u32),
    Float(f32),
    Offset(u32),
    String(u32),
}

impl From<&Byml> for NodeValue {
    fn from(node: &Byml) -> NodeValue {
        match node {
            Byml::Int(i) => NodeValue::Int(*i),
            Byml::UInt(u) => NodeValue::UInt(*u),
            Byml::Float(f) => NodeValue::Float(f.into()),
            Byml::Bool(b) => NodeValue::Bool(*b),
            Byml::String(_) => NodeValue::String(0),
            _ => NodeValue::Offset(0),
        }
    }
}

impl BinWrite for NodeValue {
    fn write_options<W: Write>(
        &self,
        writer: &mut W,
        options: &WriterOption,
    ) -> Result<(), std::io::Error> {
        match self {
            NodeValue::Bool(v) => (if *v { 1u32 } else { 0u32 }).write_options(writer, options),
            NodeValue::Int(v) => v.write_options(writer, options),
            NodeValue::UInt(v) | NodeValue::Offset(v) => v.write_options(writer, options),
            NodeValue::Float(v) => v.write_options(writer, options),
            NodeValue::String(v) => v.write_options(writer, options),
        }
    }
}

#[derive(Debug, BinWrite)]
struct StringTable {
    entries: U24,
    offsets: Vec<u32>,
}

#[derive(Debug, BinWrite)]
struct AlignedCStr {
    #[binwrite(cstr, align(4))]
    string: String,
}

struct BymlWriter<'a, W: Write + Seek> {
    data: &'a Byml,
    writer: &'a mut W,
    opts: WriterOption,
    version: u16,
    keys: IndexSet<String>,
    strings: IndexSet<String>,
    written_nodes: IndexMap<u64, u32>,
}

#[inline]
fn calculate_hash(t: &Byml) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

fn collect_strings(data: &Byml) -> IndexSet<String> {
    let mut strs: IndexSet<String> = IndexSet::new();
    match data {
        Byml::String(v) => {
            strs.insert(v.to_owned());
        }
        Byml::Array(v) => strs.par_extend(v.par_iter().flat_map(|x: &Byml| collect_strings(x))),
        Byml::Hash(v) => strs.par_extend(v.par_iter().flat_map(|(_, v)| collect_strings(v))),
        _ => (),
    };
    strs.par_sort();
    strs
}

fn collect_keys(data: &Byml) -> IndexSet<String> {
    let mut keys: IndexSet<String> = IndexSet::new();
    match data {
        Byml::Hash(v) => {
            keys.par_extend(v.par_iter().map(|(k, _)| k.to_owned()));
            keys.par_extend(v.par_iter().flat_map(|(_, v)| collect_keys(v)))
        }
        Byml::Array(v) => keys.par_extend(v.par_iter().flat_map(|x| collect_keys(x))),
        _ => (),
    }
    keys.par_sort();
    keys
}

impl<W: Write + Seek> BymlWriter<'_, W> {
    fn new<'a>(
        writer: &'a mut W,
        data: &'a Byml,
        endian: binwrite::Endian,
        version: u16,
    ) -> BymlWriter<'a, W> {
        BymlWriter {
            writer,
            data,
            opts: binwrite::writer_option_new!(endian: endian),
            version,
            strings: collect_strings(data),
            keys: collect_keys(data),
            written_nodes: IndexMap::new(),
        }
    }

    #[inline]
    fn write<B: BinWrite>(&mut self, val: &B) -> WriteResult {
        val.write_options(self.writer, &self.opts)?;
        Ok(())
    }

    fn write_string_table(&mut self, strings: &IndexSet<String>) -> WriteResult {
        let start_pos = self.writer.stream_position()?;
        self.write(&NodeType::StringTable)?;
        self.write(&U24(strings.len() as u64))?;
        fn gen_str_offsets(x: &IndexSet<String>) -> Vec<u32> {
            let mut offsets: Vec<u32> = vec![];
            let mut pos = 4 + ((x.len() + 1) as u32 * 4);
            for string in x.iter() {
                offsets.push(pos);
                pos += string.len() as u32 + 1;
                pos = ((pos as i32 + 3) & -4) as u32;
            }
            offsets.push(pos);
            offsets
        }
        let offsets = gen_str_offsets(strings);
        self.write(&offsets)?;
        self.align_cursor()?;
        for (i, s) in strings.iter().enumerate() {
            self.writer
                .seek(SeekFrom::Start(start_pos + offsets[i] as u64))?;
            self.write(s)?;
            self.write(&0u8)?;
        }
        self.align_cursor()?;
        Ok(())
    }

    fn write_doc(&mut self) -> WriteResult {
        if !self.data.is_container() {
            return Err(WriteError(format!(
                "Root node must be a hash or array, not {:?}",
                self.data.get_type()
            )));
        }
        let mut header = Header {
            magic: match self.opts.endian {
                binwrite::Endian::Big => *b"BY",
                binwrite::Endian::Little => *b"YB",
                _ => unreachable!(),
            },
            version: self.version,
            hash_table_offset: 0x0,
            string_table_offset: 0x0,
            root_node_offset: 0x0,
        };
        self.writer.seek(SeekFrom::Start(0x10))?;
        if !self.keys.is_empty() {
            header.hash_table_offset = self.writer.stream_position()? as u32;
            self.write_string_table(&self.keys.clone())?;
            self.align_cursor()?;
        }
        if !self.strings.is_empty() {
            header.string_table_offset = self.writer.stream_position()? as u32;
            self.write_string_table(&self.strings.clone())?;
            self.align_cursor()?;
        }
        header.root_node_offset = self.writer.stream_position()? as u32;
        self.writer.seek(SeekFrom::Start(0))?;
        self.write(&header)?;
        self.writer
            .seek(SeekFrom::Start(header.root_node_offset.into()))?;
        self.write_offset_node(&self.data)?;
        Ok(())
    }

    fn write_offset_node(&mut self, node: &Byml) -> WriteResult {
        let pos = self.writer.stream_position()?;
        match node {
            Byml::Hash(v) => self.write_hash(v),
            Byml::Array(v) => self.write_array(v),
            Byml::Double(v) => {
                let dbl: f64 = v.into();
                self.write(&(dbl))
            }
            Byml::Int64(v) => self.write(v),
            Byml::UInt64(v) => self.write(v),
            Byml::Binary(v) => {
                self.write(&(v.len() as u32))?;
                self.write(v)
            }
            _ => Err(WriteError(format!(
                "Node {:?} is not a valid offset node",
                node
            ))),
        }?;
        self.written_nodes.insert(calculate_hash(node), pos as u32);
        Ok(())
    }

    fn write_hash(&mut self, hash: &BTreeMap<String, Byml>) -> WriteResult {
        let start_pos = self.writer.stream_position()?;
        let mut after_nodes: IndexMap<usize, &Byml> = IndexMap::new();
        let mut hash_node = HashNode {
            count: U24(hash.len() as u64),
            entries: hash
                .iter()
                .enumerate()
                .map(|(i, (k, v))| {
                    let mut entry = HashEntry {
                        key_idx: U24(self.keys.get_index_of(k).unwrap() as u64),
                        r#type: v.get_type(),
                        value: NodeValue::from(v),
                    };
                    if !v.is_value() && !v.is_string() {
                        after_nodes.insert(i, v);
                    }
                    if let Byml::String(s) = v {
                        entry.value =
                            NodeValue::String(self.strings.get_index_of(s).unwrap() as u32)
                    }
                    entry
                })
                .collect::<Vec<HashEntry>>(),
        };
        self.writer
            .seek(SeekFrom::Current((hash.len() as i64 * 8) + 4))?;
        for (i, b) in after_nodes.into_iter() {
            match self.written_nodes.get(&calculate_hash(b)) {
                Some(off) => hash_node.entries[i].value = NodeValue::Offset(*off),
                None => {
                    hash_node.entries[i].value =
                        NodeValue::Offset(self.writer.stream_position()? as u32);
                    self.write_offset_node(&b)?;
                    self.align_cursor()?;
                }
            }
        }
        let end_pos = self.writer.stream_position()?;
        self.writer.seek(SeekFrom::Start(start_pos))?;
        self.write(&NodeType::Hash)?;
        self.write(&hash_node)?;
        self.writer.seek(SeekFrom::Start(end_pos))?;
        Ok(())
    }

    fn write_array(&mut self, array: &[Byml]) -> WriteResult {
        let start_pos = self.writer.stream_position()?;
        let mut after_nodes: IndexMap<usize, &Byml> = IndexMap::new();
        let array_node = ArrayNode {
            count: U24(array.len() as u64),
            types: array.par_iter().map(|x| x.get_type()).collect(),
        };
        let mut array_values = array
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let mut val = NodeValue::from(v);
                if !v.is_value() && !v.is_string() {
                    after_nodes.insert(i, v);
                }
                if let Byml::String(s) = v {
                    val = NodeValue::String(self.strings.get_index_of(s).unwrap() as u32)
                }
                val
            })
            .collect::<Vec<NodeValue>>();
        self.writer.seek(SeekFrom::Current(
            (array.len() as i64) + (array.len() as i64 * 4) + 4,
        ))?;
        self.align_cursor()?;
        for (i, b) in after_nodes.into_iter() {
            match self.written_nodes.get(&calculate_hash(b)) {
                Some(off) => array_values[i] = NodeValue::Offset(*off),
                None => {
                    array_values[i] = NodeValue::Offset(self.writer.stream_position()? as u32);
                    self.write_offset_node(&b)?;
                    self.align_cursor()?;
                }
            }
        }
        let end_pos = self.writer.stream_position()?;
        self.writer.seek(SeekFrom::Start(start_pos))?;
        self.write(&NodeType::Array)?;
        self.write(&array_node)?;
        self.align_cursor()?;
        self.write(&array_values)?;
        self.writer.seek(SeekFrom::Start(end_pos))?;
        Ok(())
    }

    fn align_cursor(&mut self) -> WriteResult {
        let aligned_pos = ((self.writer.stream_position()? as i64 + 3) & -4) as u64;
        self.writer.seek(SeekFrom::Start(aligned_pos))?;
        Ok(())
    }
}

impl Into<u8> for &NodeType {
    fn into(self) -> u8 {
        match self {
            NodeType::String => 0xA0,
            NodeType::Binary => 0xA1,
            NodeType::Array => 0xC0,
            NodeType::Hash => 0xC1,
            NodeType::Bool => 0xD0,
            NodeType::Int => 0xD1,
            NodeType::Float => 0xD2,
            NodeType::UInt => 0xD3,
            NodeType::Int64 => 0xD4,
            NodeType::UInt64 => 0xD5,
            NodeType::Double => 0xD6,
            NodeType::Null => 0xFF,
            NodeType::StringTable => 0xC2,
        }
    }
}

impl BinWrite for NodeType {
    fn write_options<W: Write>(
        self: &NodeType,
        writer: &mut W,
        _options: &WriterOption,
    ) -> Result<(), std::io::Error> {
        let v: u8 = self.into();
        v.write(writer)
    }
}

impl BinWrite for U24 {
    fn write_options<W: Write>(
        &self,
        writer: &mut W,
        options: &WriterOption,
    ) -> Result<(), std::io::Error> {
        let mut buf: [u8; 3] = [0; 3];
        match options.endian {
            binwrite::Endian::Big => BigEndian::write_uint(&mut buf, self.0, 3),
            binwrite::Endian::Little => LittleEndian::write_uint(&mut buf, self.0, 3),
            _ => unreachable!(),
        };
        writer.write_all(&buf)?;
        Ok(())
    }
}
