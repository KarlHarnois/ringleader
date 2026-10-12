pub struct RenameRequest {
    old_name: String,
    new_name: String,
    keep_branch: bool,
}

impl RenameRequest {
    pub fn new(old_name: &str, new_name: &str, keep_branch: bool) -> Self {
        Self {
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
            keep_branch,
        }
    }

    pub fn old_name(&self) -> &str {
        &self.old_name
    }

    pub fn new_name(&self) -> &str {
        &self.new_name
    }

    pub fn keeps_branch(&self) -> bool {
        self.keep_branch
    }
}
