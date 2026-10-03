pub trait Clock {
    fn now_epoch_seconds(&self) -> u64;
}
