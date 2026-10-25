pub trait BatchReport {
    fn summary(&self) -> String;

    fn needs_attention(&self) -> bool;
}
