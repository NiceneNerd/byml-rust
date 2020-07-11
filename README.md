# Nintendo BYML (binary YAML) library in Rust

[![crates.io](https://img.shields.io/crates/v/byml)](https://crates.io/crates/byml)
[![api](https://img.shields.io/badge/api-rustdoc-558b2f)](https://docs.rs/byml/)
[![license](https://img.shields.io/crates/l/byml)](https://spdx.org/licenses/MIT.html)

A simple to use library for reading, writing, and converting Nintendo binary YAML (BYML) files in
Rust. Supports BYML versions 2-4, (v2 used in *The Legend of Zelda: Breath of the Wild*). Can
convert from BYML to readable, editable YAML and back.

Sample usage:

```rust
// First grab the file bytes. Yaz0 compressed files are automatically decompressed.
let bytes: Vec<u8> = std::fs::read("ActorInfo.product.byml").unwrap();
// Parse the data as a Byml document
let actor_info: Byml = Byml::from_binary(&bytes).unwrap();
// Index BYML hashes and arrays naturally
let actor_list: &Vec<Byml> = actor_info["Actors"].as_array().unwrap();
// 7934 actors, egads!
assert_eq!(actor_list.len(), 7934);
// Hmm, we'll iterate the actors listed in this file:
for actor in actor_list.iter() {
    // Print each actor's name
    println!("{}", actor["name"].as_string().unwrap());
}
// Dump to YAML
std::fs::write("ActorInfo.product.yml", actor_info.to_text().unwrap()).unwrap();
```

## License Notice

This software contains heavily edited code from [`yaml-rust`](https://crates.io/crates/yaml-rust),
mostly to simplify parsing and emitting BYML text representation. The original MIT/Apache license
and code are available on the [GitHub repo](https://github.com/chyh1990/yaml-rust).
