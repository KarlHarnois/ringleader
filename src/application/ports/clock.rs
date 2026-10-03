pub trait Clock: Send + Sync {
    fn now_epoch_seconds(&self) -> u64;
}
