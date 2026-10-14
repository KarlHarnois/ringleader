pub trait Interrupts {
    fn raised(&self) -> Option<i32>;
}
