use serde::{Serialize, Deserialize};
use serde_big_array::BigArray;
use bincode;

#[derive(Serialize, Deserialize)]
struct S {
    #[serde(with = "BigArray")]
    data: [u8; 16384],
}

fn main() {
    let s = S { data: [42; 16384] };
    let encoded = bincode::serde::encode_to_vec(&s, bincode::config::standard()).unwrap();
    let (_decoded, _) = bincode::serde::decode_from_slice::<S, _>(&encoded, bincode::config::standard()).unwrap();
    println!("decoded {} bytes", _decoded.data.len());
}
