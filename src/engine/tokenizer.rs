/// Estimate token count from byte size.
/// Uses bytes / 4 heuristic matching token-police.py.
pub fn estimate_tokens(bytes: u64) -> u64 {
    bytes / 4
}
