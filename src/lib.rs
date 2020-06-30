#![feature(seek_convenience)]
use binread::BinRead;
use std::collections::BTreeMap;
use std::error::Error;

mod parse;
mod write;

type AnyError = Box<dyn Error>;
#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub enum Endian {
    Big,
    Little,
}

impl From<binread::Endian> for Endian {
    fn from(endian: binread::Endian) -> Endian {
        match endian {
            binread::Endian::Big => Endian::Big,
            binread::Endian::Little => Endian::Little,
            _ => unimplemented!(),
        }
    }
}

impl Into<binwrite::Endian> for Endian {
    fn into(self) -> binwrite::Endian {
        match self {
            Endian::Big => binwrite::Endian::Big,
            Endian::Little => binwrite::Endian::Little,
        }
    }
}

#[derive(Debug)]
pub struct TypeError;

impl Error for TypeError {}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Incorrect type ")
    }
}

#[repr(u8)]
#[derive(Debug, BinRead, PartialEq)]
pub enum NodeType {
    String = 0xA0,
    Binary = 0xA1,
    Array = 0xC0,
    Hash = 0xC1,
    StringTable = 0xC2,
    Bool = 0xD0,
    Int = 0xD1,
    Float = 0xD2,
    UInt = 0xD3,
    Int64 = 0xD4,
    UInt64 = 0xD5,
    Double = 0xD6,
    Null = 0xFF,
}

#[derive(Debug, PartialEq)]
struct U24(u64);
#[derive(Debug, PartialEq, Eq, Clone, Hash, Copy)]
pub struct Float(u32, Endian);
#[derive(Debug, PartialEq, Eq, Clone, Hash, Copy)]
pub struct Double(u64, Endian);

impl Into<f32> for &Float {
    fn into(self) -> f32 {
        match self.1 {
            Endian::Big => f32::from_be_bytes(self.0.to_be_bytes()),
            Endian::Little => f32::from_le_bytes(self.0.to_le_bytes()),
        }
    }
}

impl Into<f64> for &Double {
    fn into(self) -> f64 {
        match self.1 {
            Endian::Big => f64::from_be_bytes(self.0.to_be_bytes()),
            Endian::Little => f64::from_le_bytes(self.0.to_le_bytes()),
        }
    }
}

#[derive(Debug, Clone, Eq, Hash)]
pub enum Byml {
    Null,
    String(String),
    Binary(Vec<u8>),
    Array(Vec<Byml>),
    Hash(BTreeMap<String, Byml>),
    Bool(bool),
    Int(i32),
    Float(Float),
    UInt(u32),
    Int64(i64),
    UInt64(u64),
    Double(Double),
}

impl PartialEq for Byml {
    fn eq(&self, other: &Byml) -> bool {
        match self {
            Byml::Array(a) => match other.as_array() {
                Ok(a2) => a == a2,
                Err(_) => false,
            },
            Byml::Hash(h) => match other.as_hash() {
                Ok(h2) => h == h2,
                Err(_) => false,
            },
            Byml::Binary(v) => match other.as_binary() {
                Ok(v2) => v == v2,
                Err(_) => false,
            },
            Byml::Bool(v) => match other.as_bool() {
                Ok(v2) => v == v2,
                Err(_) => false,
            },
            Byml::Double(v) => match other.as_double() {
                Ok(v2) => {
                    let v1: f64 = v.into();
                    v1 == v2
                }
                Err(_) => false,
            },
            Byml::Float(v) => match other.as_float() {
                Ok(v2) => {
                    let v1: f32 = v.into();
                    v1 == v2
                }
                Err(_) => false,
            },
            Byml::Int(v) => match other.as_int() {
                Ok(v2) => v == v2,
                Err(_) => false,
            },
            Byml::Int64(v) => match other.as_int64() {
                Ok(v2) => v == v2,
                Err(_) => false,
            },
            Byml::UInt(v) => match other.as_uint() {
                Ok(v2) => v == v2,
                Err(_) => false,
            },
            Byml::UInt64(v) => match other.as_uint64() {
                Ok(v2) => v == v2,
                Err(_) => false,
            },
            Byml::String(v) => match other.as_string() {
                Ok(v2) => v == v2,
                Err(_) => false,
            },
            Byml::Null => other.is_null(),
        }
    }
}

pub enum BymlIndex<'a> {
    Key(&'a str),
    Index(usize),
}

impl From<&'static str> for BymlIndex<'_> {
    fn from(key: &'static str) -> BymlIndex<'_> {
        BymlIndex::Key(key)
    }
}

impl From<usize> for BymlIndex<'_> {
    fn from(idx: usize) -> BymlIndex<'static> {
        BymlIndex::Index(idx)
    }
}

impl<'a, I> std::ops::Index<I> for Byml
where
    I: Into<BymlIndex<'a>>,
{
    type Output = Byml;
    fn index(&self, index: I) -> &Self::Output {
        let idx = index.into();
        match idx {
            BymlIndex::Key(k) => &self.as_hash().unwrap()[k],
            BymlIndex::Index(i) => &self.as_array().unwrap()[i],
        }
    }
}

impl Byml {
    pub fn is_container(&self) -> bool {
        match self {
            Byml::Hash(_) | Byml::Array(_) => true,
            _ => false,
        }
    }

    pub fn is_value(&self) -> bool {
        match self {
            Byml::Int(_) | Byml::UInt(_) | Byml::Float(_) | Byml::Bool(_) => true,
            _ => false,
        }
    }

    pub fn is_string(&self) -> bool {
        match self {
            Byml::String(_) => true,
            _ => false,
        }
    }

    pub fn get_type(&self) -> NodeType {
        match self {
            Byml::Array(_) => NodeType::Array,
            Byml::Hash(_) => NodeType::Hash,
            Byml::Binary(_) => NodeType::Binary,
            Byml::Bool(_) => NodeType::Bool,
            Byml::Double(_) => NodeType::Double,
            Byml::Float(_) => NodeType::Float,
            Byml::Int(_) => NodeType::Int,
            Byml::Int64(_) => NodeType::Int64,
            Byml::Null => NodeType::Null,
            Byml::String(_) => NodeType::String,
            Byml::UInt(_) => NodeType::UInt,
            Byml::UInt64(_) => NodeType::UInt64,
        }
    }

    pub fn as_hash(&self) -> Result<&BTreeMap<String, Byml>, TypeError> {
        match self {
            Byml::Hash(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_array(&self) -> Result<&Vec<Byml>, TypeError> {
        match self {
            Byml::Array(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_binary(&self) -> Result<&Vec<u8>, TypeError> {
        match self {
            Byml::Binary(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_bool(&self) -> Result<&bool, TypeError> {
        match self {
            Byml::Bool(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_string(&self) -> Result<&String, TypeError> {
        match self {
            Byml::String(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_int(&self) -> Result<&i32, TypeError> {
        match self {
            Byml::Int(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_int64(&self) -> Result<&i64, TypeError> {
        match self {
            Byml::Int64(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_uint(&self) -> Result<&u32, TypeError> {
        match self {
            Byml::UInt(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_uint64(&self) -> Result<&u64, TypeError> {
        match self {
            Byml::UInt64(v) => Ok(&v),
            _ => Err(TypeError),
        }
    }

    pub fn as_float(&self) -> Result<f32, TypeError> {
        match self {
            Byml::Float(v) => Ok(v.into()),
            _ => Err(TypeError),
        }
    }

    pub fn as_double(&self) -> Result<f64, TypeError> {
        match self {
            Byml::Double(v) => Ok(v.into()),
            _ => Err(TypeError),
        }
    }

    pub fn is_null(&self) -> bool {
        match self {
            Byml::Null => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Byml;
    use glob::glob;
    use std::fs::read;
    use std::path::PathBuf;

    #[test]
    fn parse_byml() {
        let data = read("test/ActorInfo.product.sbyml").unwrap();
        let actorinfo = Byml::from_binary(&data).unwrap();
        println!("{:?}", actorinfo["Actors"][1]);
        assert_eq!(actorinfo["Actors"].as_array().unwrap().len(), 7934);
        let data = read("test/A-1_Static.mubin").unwrap();
        Byml::from_binary(&data).unwrap();
    }

    #[test]
    fn binary_roundtrip() {
        for file in glob("test/*.*").unwrap() {
            let good_file: PathBuf = file.unwrap();
            let data = read(&good_file).unwrap();
            let byml = Byml::from_binary(&data).unwrap();
            let new_byml =
                Byml::from_binary(&byml.to_binary(crate::Endian::Little, 2).unwrap()).unwrap();
            assert_eq!(byml, new_byml);
        }
    }
}
