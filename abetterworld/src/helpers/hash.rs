use xxhash_rust::xxh3::xxh3_64;

pub fn hash_uri(uri: &str) -> u64 {
    xxh3_64(uri.as_bytes())
}
