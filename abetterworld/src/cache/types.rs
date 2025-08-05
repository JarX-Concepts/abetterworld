use bytes::Bytes;

pub trait TilesetMemoryCache: Send + Sync {
    fn get(&self, key: u64) -> Option<(String, Bytes)>;
    fn insert(&self, key: u64, value: (String, Bytes));
    fn invalidate_all(&self);
}
