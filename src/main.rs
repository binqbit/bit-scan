mod scan_v1;
pub mod utils;

fn main() {
    let args = std::env::args().collect::<Vec<String>>();
    let pubkey = args.get(1).expect("Usage: bit-scan <public_key> <bits>");
    let bits = args
        .get(2)
        .expect("Usage: bit-scan <public_key> <bits>")
        .parse::<u32>()
        .expect("Bits must be a valid number");
    let is_view = args.iter().any(|arg| arg == "--view");

    scan_v1::scan(pubkey, bits, is_view);
}
