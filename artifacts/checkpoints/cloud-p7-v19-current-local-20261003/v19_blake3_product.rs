fn main() {
    let path = std::env::args().nth(1).unwrap();
    let bytes = std::fs::read(path).unwrap();
    println!("{}", blake3::hash(&bytes).to_hex());
}
