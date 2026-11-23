/// Continues a 64-bit FNV-1a hash with one length-prefixed definition.
pub(super) fn definition(hash: u64, definition: &str) -> u64 {
    let hash = extend(hash, &(definition.len() as u64).to_le_bytes());
    extend(hash, definition.as_bytes())
}

fn extend(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}
