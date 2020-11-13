use super::forked::parser::*;
use super::forked::scanner::{Marker, ScanError, TokenType};
use crate::Byml;
use std::collections::BTreeMap;
use std::error::Error;
use std::f64;
use std::i64;
use std::mem;

impl Byml {
    /// Read a BYML document from a YAML string. The input YAML format is the same as that used
    /// by the `byml` and `oead` Python libraries.
    pub fn from_text(text: &str) -> Result<Byml, Box<dyn Error>> {
        let mut result = BymlLoader::load_from_str(text)?;
        Ok(std::mem::take(
            result.get_mut(0).ok_or("No document parsed")?,
        ))
    }
}

type Hash = BTreeMap<String, Byml>;

#[derive(Debug)]
pub struct BymlLoader {
    docs: Vec<Byml>,
    doc_stack: Vec<(Byml, usize)>,
    key_stack: Vec<String>,
}

impl MarkedEventReceiver for BymlLoader {
    fn on_event(&mut self, ev: Event, _: Marker) {
        // println!("EV {:?}", ev);
        match ev {
            Event::DocumentStart => {
                // do nothing
            }
            Event::DocumentEnd => {
                match self.doc_stack.len() {
                    // empty document
                    0 => self.docs.push(Byml::Null),
                    1 => self.docs.push(self.doc_stack.pop().unwrap().0),
                    _ => unreachable!(),
                }
            }
            Event::SequenceStart(aid, _) => {
                self.doc_stack.push((Byml::Array(Vec::new()), aid));
            }
            Event::SequenceEnd => {
                let node = self.doc_stack.pop().unwrap();
                self.insert_new_node(node);
            }
            Event::MappingStart(aid, _) => {
                self.doc_stack.push((Byml::Hash(Hash::new()), aid));
                self.key_stack.push(String::new());
            }
            Event::MappingEnd => {
                self.key_stack.pop().unwrap();
                let node = self.doc_stack.pop().unwrap();
                self.insert_new_node(node);
            }
            Event::Scalar(v, _style, aid, tag) => {
                let node = if let Some(TokenType::Tag(ref handle, ref suffix)) = tag {
                    if handle == "!!" {
                        match suffix.as_ref() {
                            "bool" => {
                                // "true" or "false"
                                match v.parse::<bool>() {
                                    Err(_) => Byml::Null,
                                    Ok(v) => Byml::Bool(v),
                                }
                            }
                            "int" => match v.parse::<i32>() {
                                Err(_) => Byml::Null,
                                Ok(v) => Byml::Int(v),
                            },
                            "float" => match v.parse::<f32>() {
                                Ok(v) => Byml::Float(v.into()),
                                Err(_) => Byml::Null,
                            },
                            "null" => match v.as_ref() {
                                "~" | "null" => Byml::Null,
                                _ => Byml::Null,
                            },
                            "binary" => {
                                match base64::decode_config(
                                    v.split_whitespace().collect::<String>().as_bytes(),
                                    base64::STANDARD_NO_PAD,
                                ) {
                                    Ok(v) => Byml::Binary(v),
                                    Err(e) => Byml::String(format!("{:?}", e)),
                                }
                            }
                            _ => Byml::String(v),
                        }
                    } else if handle == "!" {
                        match suffix.as_ref() {
                            "u" => match parse_int::parse::<u32>(v.as_ref()) {
                                Ok(v) => Byml::UInt(v),
                                Err(_) => Byml::Null,
                            },
                            "l" => match v.parse::<i64>() {
                                Ok(v) => Byml::Int64(v),
                                Err(_) => Byml::Null,
                            },
                            "f64" => match v.parse::<f64>() {
                                Ok(v) => Byml::Double(v.into()),
                                Err(_) => Byml::Null,
                            },
                            "ul" => match v.parse::<u64>() {
                                Ok(v) => Byml::UInt64(v),
                                Err(_) => Byml::Null,
                            },
                            "binary" => {
                                match base64::decode_config(
                                    v.split_whitespace().collect::<String>().as_bytes(),
                                    base64::STANDARD_NO_PAD,
                                ) {
                                    Ok(v) => Byml::Binary(v),
                                    Err(e) => Byml::String(format!("{:?}", e)),
                                }
                            }
                            _ => Byml::String(v),
                        }
                    } else {
                        Byml::String(v)
                    }
                } else {
                    match v.parse::<i32>() {
                        Ok(v) => Byml::Int(v),
                        Err(_) => match v.parse::<f32>() {
                            Ok(v) => Byml::Float(v.into()),
                            Err(_) => match v.as_ref() {
                                "true" => Byml::Bool(true),
                                "false" => Byml::Bool(false),
                                _ => Byml::String(v),
                            },
                        },
                    }
                };

                self.insert_new_node((node, aid));
            }
            Event::Alias(_) => (),
            _ => { /* ignore */ }
        }
    }
}

impl BymlLoader {
    fn insert_new_node(&mut self, mut node: (Byml, usize)) {
        if self.doc_stack.is_empty() {
            self.doc_stack.push(node);
        } else {
            let parent = self.doc_stack.last_mut().unwrap();
            match *parent {
                (Byml::Array(ref mut v), _) => v.push(node.0),
                (Byml::Hash(ref mut h), _) => {
                    let cur_key = self.key_stack.last_mut().unwrap();
                    // current node is a key
                    if cur_key.as_bytes() == b"" {
                        *cur_key = match node.0.as_mut_string() {
                            Ok(v) => std::mem::take(v),
                            Err(_) => node.0.as_int().unwrap().to_string(),
                        };
                    // current node is a value
                    } else {
                        let mut newkey = String::new();
                        mem::swap(&mut newkey, cur_key);
                        h.insert(newkey, node.0);
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    pub fn load_from_str(source: &str) -> Result<Vec<Byml>, ScanError> {
        let mut loader = BymlLoader {
            docs: Vec::new(),
            doc_stack: Vec::new(),
            key_stack: Vec::new(),
        };
        let mut parser = Parser::new(source.chars());
        parser.load(&mut loader, true)?;
        Ok(loader.docs)
    }
}
