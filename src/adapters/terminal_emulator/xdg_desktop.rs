use super::desktop::Desktop;

pub struct XdgDesktop;

impl XdgDesktop {
    pub fn current() -> Desktop {
        Desktop::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linux_desktop_refuses_no_window_or_browser() {
        assert_eq!(XdgDesktop::current().refusal(), None);
    }
}
