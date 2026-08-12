//! Errno type and result alias used throughout the sandbox.
//!
//! Handlers return `SysResult<T>`; a `SysError` carries a Linux errno that the
//! supervisor turns into a seccomp error reply. This replaces the Zig
//! `LinuxErr` error set + `checkErr`/`toLinuxE` helpers.

use std::fmt;

/// A Linux errno. Values match `<errno.h>` on all Linux architectures we target
/// (they share the asm-generic numbering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
#[allow(clippy::upper_case_acronyms)]
pub enum Errno {
    PERM = 1,
    NOENT = 2,
    SRCH = 3,
    INTR = 4,
    IO = 5,
    NXIO = 6,
    E2BIG = 7,
    NOEXEC = 8,
    BADF = 9,
    CHILD = 10,
    AGAIN = 11,
    NOMEM = 12,
    ACCES = 13,
    FAULT = 14,
    NOTBLK = 15,
    BUSY = 16,
    EXIST = 17,
    XDEV = 18,
    NODEV = 19,
    NOTDIR = 20,
    ISDIR = 21,
    INVAL = 22,
    NFILE = 23,
    MFILE = 24,
    NOTTY = 25,
    TXTBSY = 26,
    FBIG = 27,
    NOSPC = 28,
    SPIPE = 29,
    ROFS = 30,
    MLINK = 31,
    PIPE = 32,
    DOM = 33,
    RANGE = 34,
    DEADLK = 35,
    NAMETOOLONG = 36,
    NOLCK = 37,
    NOSYS = 38,
    NOTEMPTY = 39,
    LOOP = 40,
    NOMSG = 42,
    IDRM = 43,
    NOTSOCK = 88,
    DESTADDRREQ = 89,
    MSGSIZE = 90,
    PROTOTYPE = 91,
    NOPROTOOPT = 92,
    PROTONOSUPPORT = 93,
    OPNOTSUPP = 95,
    AFNOSUPPORT = 97,
    ADDRINUSE = 98,
    ADDRNOTAVAIL = 99,
    NETDOWN = 100,
    NETUNREACH = 101,
    CONNRESET = 104,
    NOBUFS = 105,
    ISCONN = 106,
    NOTCONN = 107,
    SHUTDOWN = 108,
    TIMEDOUT = 110,
    CONNREFUSED = 111,
    HOSTUNREACH = 113,
    ALREADY = 114,
    INPROGRESS = 115,
    STALE = 116,
    OVERFLOW = 75,
    CANCELED = 125,
}

impl Errno {
    /// The raw errno number.
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Build an `Errno` from a raw errno number, if it is one we model.
    pub fn from_raw(code: i32) -> Option<Errno> {
        use Errno::*;
        Some(match code {
            1 => PERM,
            2 => NOENT,
            3 => SRCH,
            4 => INTR,
            5 => IO,
            6 => NXIO,
            7 => E2BIG,
            8 => NOEXEC,
            9 => BADF,
            10 => CHILD,
            11 => AGAIN,
            12 => NOMEM,
            13 => ACCES,
            14 => FAULT,
            15 => NOTBLK,
            16 => BUSY,
            17 => EXIST,
            18 => XDEV,
            19 => NODEV,
            20 => NOTDIR,
            21 => ISDIR,
            22 => INVAL,
            23 => NFILE,
            24 => MFILE,
            25 => NOTTY,
            26 => TXTBSY,
            27 => FBIG,
            28 => NOSPC,
            29 => SPIPE,
            30 => ROFS,
            31 => MLINK,
            32 => PIPE,
            33 => DOM,
            34 => RANGE,
            35 => DEADLK,
            36 => NAMETOOLONG,
            37 => NOLCK,
            38 => NOSYS,
            39 => NOTEMPTY,
            40 => LOOP,
            42 => NOMSG,
            43 => IDRM,
            75 => OVERFLOW,
            88 => NOTSOCK,
            89 => DESTADDRREQ,
            90 => MSGSIZE,
            91 => PROTOTYPE,
            92 => NOPROTOOPT,
            93 => PROTONOSUPPORT,
            95 => OPNOTSUPP,
            97 => AFNOSUPPORT,
            98 => ADDRINUSE,
            99 => ADDRNOTAVAIL,
            100 => NETDOWN,
            101 => NETUNREACH,
            104 => CONNRESET,
            105 => NOBUFS,
            106 => ISCONN,
            107 => NOTCONN,
            108 => SHUTDOWN,
            110 => TIMEDOUT,
            111 => CONNREFUSED,
            113 => HOSTUNREACH,
            114 => ALREADY,
            115 => INPROGRESS,
            116 => STALE,
            125 => CANCELED,
            _ => return None,
        })
    }
}

/// Error carrying a Linux errno.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysError(pub Errno);

impl SysError {
    pub fn errno(self) -> Errno {
        self.0
    }
}

impl From<Errno> for SysError {
    fn from(e: Errno) -> Self {
        SysError(e)
    }
}

impl fmt::Display for SysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} (errno {})", self.0, self.0.code())
    }
}

impl std::error::Error for SysError {}

pub type SysResult<T> = Result<T, SysError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_common_errnos() {
        for e in [
            Errno::PERM,
            Errno::NOENT,
            Errno::BADF,
            Errno::INVAL,
            Errno::NOSYS,
            Errno::NOTEMPTY,
            Errno::NOTSOCK,
        ] {
            assert_eq!(Errno::from_raw(e.code()), Some(e));
        }
    }

    #[test]
    fn unknown_errno_is_none() {
        assert_eq!(Errno::from_raw(9999), None);
    }

    #[test]
    fn syserror_wraps_errno() {
        let err: SysError = Errno::BADF.into();
        assert_eq!(err.errno(), Errno::BADF);
        assert_eq!(err.errno().code(), 9);
    }
}
