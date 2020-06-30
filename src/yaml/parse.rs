use super::forked::parser::*;
use super::forked::scanner::{Marker, ScanError, TokenType};
use crate::Byml;
use std::collections::BTreeMap;
use std::error::Error;
use std::f64;
use std::i64;
use std::mem;

impl Byml {
    pub fn from_text(text: &str) -> Result<Byml, Box<dyn Error>> {
        let mut result = BymlLoader::load_from_str(text)?;
        Ok(std::mem::take(result.get_mut(0).ok_or("Index error")?))
    }
}

type Hash = BTreeMap<String, Byml>;

pub struct BymlLoader {
    docs: Vec<Byml>,
    // states
    // (current node, anchor_id) tuple
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
                    // XXX tag:yaml.org,2002:
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
                    // Datatype is not specified, or unrecognized
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
        // println!("DOC {:?}", self.doc_stack);
    }
}

impl BymlLoader {
    fn insert_new_node(&mut self, node: (Byml, usize)) {
        // valid anchor id starts from 1
        // if node.1 > 0 {
        //     self.anchor_map.insert(node.1, node.0.clone());
        // }
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
                        *cur_key = node.0.as_string().unwrap().clone();
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

// macro_rules! define_as (
//     ($name:ident, $t:ident, $yt:ident) => (
// pub fn $name(&self) -> Option<$t> {
//     match *self {
//         Byml::$yt(v) => Some(v),
//         _ => None
//     }
// }
//     );
// );

// macro_rules! define_as_ref (
//     ($name:ident, $t:ty, $yt:ident) => (
// pub fn $name(&self) -> Option<$t> {
//     match *self {
//         Byml::$yt(ref v) => Some(v),
//         _ => None
//     }
// }
//     );
// );

// macro_rules! define_into (
//     ($name:ident, $t:ty, $yt:ident) => (
// pub fn $name(self) -> Option<$t> {
//     match self {
//         Byml::$yt(v) => Some(v),
//         _ => None
//     }
// }
//     );
// );

// impl Yaml {
//     define_as!(as_bool, bool, Boolean);
//     define_as!(as_i64, i64, Integer);

//     define_as_ref!(as_str, &str, String);
//     define_as_ref!(as_hash, &Hash, Hash);
//     define_as_ref!(as_vec, &Array, Array);

//     define_into!(into_bool, bool, Boolean);
//     define_into!(into_i64, i64, Integer);
//     define_into!(into_string, String, String);
//     define_into!(into_hash, Hash, Hash);
//     define_into!(into_vec, Array, Array);

//     pub fn is_null(&self) -> bool {
//         match *self {
//             Byml::Null => true,
//             _ => false,
//         }
//     }

//     pub fn is_badvalue(&self) -> bool {
//         match *self {
//             Byml::Null => true,
//             _ => false,
//         }
//     }

//     pub fn is_array(&self) -> bool {
//         match *self {
//             Byml::Array(_) => true,
//             _ => false,
//         }
//     }

//     pub fn as_f64(&self) -> Option<f64> {
//         match *self {
//             Byml::Real(ref v) => parse_f64(v),
//             _ => None,
//         }
//     }

//     pub fn into_f64(self) -> Option<f64> {
//         match self {
//             Byml::Real(ref v) => parse_f64(v),
//             _ => None,
//         }
//     }
// }

// #[cfg_attr(feature = "cargo-clippy", allow(should_implement_trait))]
// impl Yaml {
//     // Not implementing FromStr because there is no possibility of Error.
//     // This function falls back to Byml::String if nothing else matches.
//     pub fn from_str(v: &str) -> Yaml {
//         if v.starts_with("0x") {
//             if let Ok(i) = i64::from_str_radix(&v[2..], 16) {
//                 return Byml::Integer(i);
//             }
//         }
//         if v.starts_with("0o") {
//             if let Ok(i) = i64::from_str_radix(&v[2..], 8) {
//                 return Byml::Integer(i);
//             }
//         }
//         if v.starts_with('+') {
//             if let Ok(i) = v[1..].parse::<i64>() {
//                 return Byml::Integer(i);
//             }
//         }
//         match v {
//             "~" | "null" => Byml::Null,
//             "true" => Byml::Boolean(true),
//             "false" => Byml::Boolean(false),
//             _ if v.parse::<i64>().is_ok() => Byml::Integer(v.parse::<i64>().unwrap()),
//             // try parsing as f64
//             _ if parse_f64(v).is_some() => Byml::Real(v.to_owned()),
//             _ => Byml::String(v.to_owned()),
//         }
//     }
// }

// static BAD_VALUE: Yaml = Byml::Null;
// impl<'a> Index<&'a str> for Yaml {
//     type Output = Yaml;

//     fn index(&self, idx: &'a str) -> &Yaml {
//         let key = Byml::String(idx.to_owned());
//         match self.as_hash() {
//             Some(h) => h.get(&key).unwrap_or(&BAD_VALUE),
//             None => &BAD_VALUE,
//         }
//     }
// }

// impl Index<usize> for Yaml {
//     type Output = Yaml;

//     fn index(&self, idx: usize) -> &Yaml {
//         if let Some(v) = self.as_vec() {
//             v.get(idx).unwrap_or(&BAD_VALUE)
//         } else if let Some(v) = self.as_hash() {
//             let key = Byml::Integer(idx as i64);
//             v.get(&key).unwrap_or(&BAD_VALUE)
//         } else {
//             &BAD_VALUE
//         }
//     }
// }

// impl IntoIterator for Yaml {
//     type Item = Yaml;
//     type IntoIter = YamlIter;

//     fn into_iter(self) -> Self::IntoIter {
//         YamlIter {
//             yaml: self.into_vec().unwrap_or_else(Vec::new).into_iter(),
//         }
//     }
// }

// pub struct YamlIter {
//     yaml: vec::IntoIter<Yaml>,
// }

// impl Iterator for YamlIter {
//     type Item = Yaml;

//     fn next(&mut self) -> Option<Yaml> {
//         self.yaml.next()
//     }
// }

// #[cfg(test)]
// mod test {
//     use crate::yaml::parse::*;
//     use std::f64;
//     #[test]
//     fn test_coerce() {
//         let s = "---
// a: 1
// b: 2.2
// c: [1, 2]
// ";
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let doc = &out[0];
//         assert_eq!(doc["a"].as_i64().unwrap(), 1i64);
//         assert_eq!(doc["b"].as_f64().unwrap(), 2.2f64);
//         assert_eq!(doc["c"][1].as_i64().unwrap(), 2i64);
//         assert!(doc["d"][0].is_badvalue());
//     }

//     #[test]
//     fn test_empty_doc() {
//         let s: String = "".to_owned();
//         YamlLoader::load_from_str(&s).unwrap();
//         let s: String = "---".to_owned();
//         assert_eq!(YamlLoader::load_from_str(&s).unwrap()[0], Byml::Null);
//     }

//     #[test]
//     fn test_parser() {
//         let s: String = "
// # comment
// a0 bb: val
// a1:
//     b1: 4
//     b2: d
// a2: 4 # i'm comment
// a3: [1, 2, 3]
// a4:
//     - - a1
//       - a2
//     - 2
// a5: 'single_quoted'
// a6: \"double_quoted\"
// a7: 你好
// "
//         .to_owned();
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let doc = &out[0];
//         assert_eq!(doc["a7"].as_str().unwrap(), "你好");
//     }

//     #[test]
//     fn test_multi_doc() {
//         let s = "
// 'a scalar'
// ---
// 'a scalar'
// ---
// 'a scalar'
// ";
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         assert_eq!(out.len(), 3);
//     }

//     #[test]
//     fn test_anchor() {
//         let s = "
// a1: &DEFAULT
//     b1: 4
//     b2: d
// a2: *DEFAULT
// ";
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let doc = &out[0];
//         assert_eq!(doc["a2"]["b1"].as_i64().unwrap(), 4);
//     }

//     #[test]
//     fn test_bad_anchor() {
//         let s = "
// a1: &DEFAULT
//     b1: 4
//     b2: *DEFAULT
// ";
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let doc = &out[0];
//         assert_eq!(doc["a1"]["b2"], Byml::Null);
//     }

//     #[test]
//     fn test_github_27() {
//         // https://github.com/chyh1990/yaml-rust/issues/27
//         let s = "&a";
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let doc = &out[0];
//         assert_eq!(doc.as_str().unwrap(), "");
//     }

//     #[test]
//     fn test_plain_datatype() {
//         let s = "
// - 'string'
// - \"string\"
// - string
// - 123
// - -321
// - 1.23
// - -1e4
// - ~
// - null
// - true
// - false
// - !!str 0
// - !!int 100
// - !!float 2
// - !!null ~
// - !!bool true
// - !!bool false
// - 0xFF
// # bad values
// - !!int string
// - !!float string
// - !!bool null
// - !!null val
// - 0o77
// - [ 0xF, 0xF ]
// - +12345
// - [ true, false ]
// ";
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let doc = &out[0];

//         assert_eq!(doc[0].as_str().unwrap(), "string");
//         assert_eq!(doc[1].as_str().unwrap(), "string");
//         assert_eq!(doc[2].as_str().unwrap(), "string");
//         assert_eq!(doc[3].as_i64().unwrap(), 123);
//         assert_eq!(doc[4].as_i64().unwrap(), -321);
//         assert_eq!(doc[5].as_f64().unwrap(), 1.23);
//         assert_eq!(doc[6].as_f64().unwrap(), -1e4);
//         assert!(doc[7].is_null());
//         assert!(doc[8].is_null());
//         assert_eq!(doc[9].as_bool().unwrap(), true);
//         assert_eq!(doc[10].as_bool().unwrap(), false);
//         assert_eq!(doc[11].as_str().unwrap(), "0");
//         assert_eq!(doc[12].as_i64().unwrap(), 100);
//         assert_eq!(doc[13].as_f64().unwrap(), 2.0);
//         assert!(doc[14].is_null());
//         assert_eq!(doc[15].as_bool().unwrap(), true);
//         assert_eq!(doc[16].as_bool().unwrap(), false);
//         assert_eq!(doc[17].as_i64().unwrap(), 255);
//         assert!(doc[18].is_badvalue());
//         assert!(doc[19].is_badvalue());
//         assert!(doc[20].is_badvalue());
//         assert!(doc[21].is_badvalue());
//         assert_eq!(doc[22].as_i64().unwrap(), 63);
//         assert_eq!(doc[23][0].as_i64().unwrap(), 15);
//         assert_eq!(doc[23][1].as_i64().unwrap(), 15);
//         assert_eq!(doc[24].as_i64().unwrap(), 12345);
//         assert!(doc[25][0].as_bool().unwrap());
//         assert!(!doc[25][1].as_bool().unwrap());
//     }

//     #[test]
//     fn test_bad_hyphen() {
//         // See: https://github.com/chyh1990/yaml-rust/issues/23
//         let s = "{-";
//         assert!(YamlLoader::load_from_str(&s).is_err());
//     }

//     #[test]
//     fn test_issue_65() {
//         // See: https://github.com/chyh1990/yaml-rust/issues/65
//         let b = "\n\"ll\\\"ll\\\r\n\"ll\\\"ll\\\r\r\r\rU\r\r\rU";
//         assert!(YamlLoader::load_from_str(&b).is_err());
//     }

//     #[test]
//     fn test_bad_docstart() {
//         assert!(YamlLoader::load_from_str("---This used to cause an infinite loop").is_ok());
//         assert_eq!(
//             YamlLoader::load_from_str("----"),
//             Ok(vec![Byml::String(String::from("----"))])
//         );
//         assert_eq!(
//             YamlLoader::load_from_str("--- #here goes a comment"),
//             Ok(vec![Byml::Null])
//         );
//         assert_eq!(
//             YamlLoader::load_from_str("---- #here goes a comment"),
//             Ok(vec![Byml::String(String::from("----"))])
//         );
//     }

//     #[test]
//     fn test_plain_datatype_with_into_methods() {
//         let s = "
// - 'string'
// - \"string\"
// - string
// - 123
// - -321
// - 1.23
// - -1e4
// - true
// - false
// - !!str 0
// - !!int 100
// - !!float 2
// - !!bool true
// - !!bool false
// - 0xFF
// - 0o77
// - +12345
// - -.INF
// - .NAN
// - !!float .INF
// ";
//         let mut out = YamlLoader::load_from_str(&s).unwrap().into_iter();
//         let mut doc = out.next().unwrap().into_iter();

//         assert_eq!(doc.next().unwrap().into_string().unwrap(), "string");
//         assert_eq!(doc.next().unwrap().into_string().unwrap(), "string");
//         assert_eq!(doc.next().unwrap().into_string().unwrap(), "string");
//         assert_eq!(doc.next().unwrap().into_i64().unwrap(), 123);
//         assert_eq!(doc.next().unwrap().into_i64().unwrap(), -321);
//         assert_eq!(doc.next().unwrap().into_f64().unwrap(), 1.23);
//         assert_eq!(doc.next().unwrap().into_f64().unwrap(), -1e4);
//         assert_eq!(doc.next().unwrap().into_bool().unwrap(), true);
//         assert_eq!(doc.next().unwrap().into_bool().unwrap(), false);
//         assert_eq!(doc.next().unwrap().into_string().unwrap(), "0");
//         assert_eq!(doc.next().unwrap().into_i64().unwrap(), 100);
//         assert_eq!(doc.next().unwrap().into_f64().unwrap(), 2.0);
//         assert_eq!(doc.next().unwrap().into_bool().unwrap(), true);
//         assert_eq!(doc.next().unwrap().into_bool().unwrap(), false);
//         assert_eq!(doc.next().unwrap().into_i64().unwrap(), 255);
//         assert_eq!(doc.next().unwrap().into_i64().unwrap(), 63);
//         assert_eq!(doc.next().unwrap().into_i64().unwrap(), 12345);
//         assert_eq!(doc.next().unwrap().into_f64().unwrap(), f64::NEG_INFINITY);
//         assert!(doc.next().unwrap().into_f64().is_some());
//         assert_eq!(doc.next().unwrap().into_f64().unwrap(), f64::INFINITY);
//     }

//     #[test]
//     fn test_hash_order() {
//         let s = "---
// b: ~
// a: ~
// c: ~
// ";
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let first = out.into_iter().next().unwrap();
//         let mut iter = first.into_hash().unwrap().into_iter();
//         assert_eq!(
//             Some((Byml::String("b".to_owned()), Byml::Null)),
//             iter.next()
//         );
//         assert_eq!(
//             Some((Byml::String("a".to_owned()), Byml::Null)),
//             iter.next()
//         );
//         assert_eq!(
//             Some((Byml::String("c".to_owned()), Byml::Null)),
//             iter.next()
//         );
//         assert_eq!(None, iter.next());
//     }

//     #[test]
//     fn test_integer_key() {
//         let s = "
// 0:
//     important: true
// 1:
//     important: false
// ";
//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let first = out.into_iter().next().unwrap();
//         assert_eq!(first[0]["important"].as_bool().unwrap(), true);
//     }

//     #[test]
//     fn test_indentation_equality() {
//         let four_spaces = YamlLoader::load_from_str(
//             r#"
// hash:
//     with:
//         indentations
// "#,
//         )
//         .unwrap()
//         .into_iter()
//         .next()
//         .unwrap();

//         let two_spaces = YamlLoader::load_from_str(
//             r#"
// hash:
//   with:
//     indentations
// "#,
//         )
//         .unwrap()
//         .into_iter()
//         .next()
//         .unwrap();

//         let one_space = YamlLoader::load_from_str(
//             r#"
// hash:
//  with:
//   indentations
// "#,
//         )
//         .unwrap()
//         .into_iter()
//         .next()
//         .unwrap();

//         let mixed_spaces = YamlLoader::load_from_str(
//             r#"
// hash:
//      with:
//                indentations
// "#,
//         )
//         .unwrap()
//         .into_iter()
//         .next()
//         .unwrap();

//         assert_eq!(four_spaces, two_spaces);
//         assert_eq!(two_spaces, one_space);
//         assert_eq!(four_spaces, mixed_spaces);
//     }

//     #[test]
//     fn test_two_space_indentations() {
//         // https://github.com/kbknapp/clap-rs/issues/965

//         let s = r#"
// subcommands:
//   - server:
//     about: server related commands
// subcommands2:
//   - server:
//       about: server related commands
// subcommands3:
//  - server:
//     about: server related commands
//             "#;

//         let out = YamlLoader::load_from_str(&s).unwrap();
//         let doc = &out.into_iter().next().unwrap();

//         println!("{:#?}", doc);
//         assert_eq!(doc["subcommands"][0]["server"], Byml::Null);
//         assert!(doc["subcommands2"][0]["server"].as_hash().is_some());
//         assert!(doc["subcommands3"][0]["server"].as_hash().is_some());
//     }

//     #[test]
//     fn test_recursion_depth_check_objects() {
//         let s = "{a:".repeat(10_000) + &"}".repeat(10_000);
//         assert!(YamlLoader::load_from_str(&s).is_err());
//     }

//     #[test]
//     fn test_recursion_depth_check_arrays() {
//         let s = "[".repeat(10_000) + &"]".repeat(10_000);
//         assert!(YamlLoader::load_from_str(&s).is_err());
//     }
// }
