use std::io;

pub struct Errno;

impl Errno {
    pub fn result(returned: libc::c_int) -> io::Result<libc::c_int> {
        if returned < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(returned)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_non_negative_return_is_the_value() {
        assert_eq!(Errno::result(3).unwrap(), 3);
    }

    #[test]
    fn zero_is_a_success() {
        assert_eq!(Errno::result(0).unwrap(), 0);
    }

    #[test]
    fn a_negative_return_carries_the_error_of_the_failed_call() {
        let _ = fs::metadata("/nonexistent/errno-probe");

        let error = Errno::result(-1).unwrap_err();

        assert_eq!(error.raw_os_error(), Some(libc::ENOENT));
    }
}
