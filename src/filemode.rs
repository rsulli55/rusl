use std::fmt;
use std::fmt::Display;

/// Stores the file mode obtained from `fs::mode()` or `fs::st_mode()`.
pub struct FileMode(pub u32);

impl FileMode {
    pub fn user_execute(&self) -> bool {
        self.0 & 0o100 != 0
    }
    pub fn user_write(&self) -> bool {
        self.0 & 0o200 != 0
    }
    pub fn user_read(&self) -> bool {
        self.0 & 0o400 != 0
    }
    pub fn group_execute(&self) -> bool {
        self.0 & 0o10 != 0
    }
    pub fn group_write(&self) -> bool {
        self.0 & 0o20 != 0
    }
    pub fn group_read(&self) -> bool {
        self.0 & 0o40 != 0
    }
    pub fn other_execute(&self) -> bool {
        self.0 & 0o1 != 0
    }
    pub fn other_write(&self) -> bool {
        self.0 & 0o2 != 0
    }
    pub fn other_read(&self) -> bool {
        self.0 & 0o4 != 0
    }
    pub fn sticky_bit(&self) -> bool {
        self.0 & 0o1000 != 0
    }
    pub fn sgid_bit(&self) -> bool {
        self.0 & 0o2000 != 0
    }
    pub fn suid_bit(&self) -> bool {
        self.0 & 0o4000 != 0
    }
}

/// Displays the mode using ls character symbols. This implementation does not fully replicate
/// ls output because it will never output `S` for the setuid or setgid bits.
///
/// See this stackexchange answer for more details about `s` vs `S`
/// https://unix.stackexchange.com/a/28412
impl Display for FileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let user_write = if self.user_write() { "w" } else { "-" };
        let user_read = if self.user_read() { "r" } else { "-" };
        let user_execute = if self.suid_bit() {
            "s"
        } else if self.user_execute() {
            "x"
        } else {
            "-"
        };
        let group_write = if self.group_write() { "w" } else { "-" };
        let group_read = if self.group_read() { "r" } else { "-" };
        let group_execute = if self.sgid_bit() {
            "s"
        } else if self.group_execute() {
            "x"
        } else {
            "-"
        };
        let other_write = if self.other_write() { "w" } else { "-" };
        let other_read = if self.other_read() { "r" } else { "-" };
        let other_execute = if self.other_execute() { "x" } else { "-" };
        let sticky_bit = if self.sticky_bit() { "t" } else { "." };
        write!(
            f,
            "{}{}{}{}{}{}{}{}{}{}",
            user_write,
            user_read,
            user_execute,
            group_write,
            group_read,
            group_execute,
            other_write,
            other_read,
            other_execute,
            sticky_bit
        )
    }
}
