//! Guest<->supervisor memory bridge.
//!
//! `RealGuestMem` uses `process_vm_readv`/`process_vm_writev`. `LocalMem`
//! interprets guest addresses as pointers in the supervisor's own address
//! space so handler unit tests can pass `&mut buf as *mut _ as u64` as a
//! syscall argument — the Rust equivalent of the Zig `builtin.is_test` bridge.

use crate::error::{Errno, SysError, SysResult};

/// Reads and writes to a (possibly remote) guest address space.
///
/// Object-safe: only byte-slice and string primitives live here so
/// `Box<dyn GuestMem>` works. Typed value helpers are in [`GuestMemExt`].
pub trait GuestMem: Send + Sync {
    fn read_bytes(&self, pid: i32, addr: u64, buf: &mut [u8]) -> SysResult<()>;
    fn write_bytes(&self, pid: i32, addr: u64, data: &[u8]) -> SysResult<()>;

    /// Read a NUL-terminated string into `buf`, returning the byte length up to
    /// (not including) the NUL. Errors with RANGE if no NUL is found.
    fn read_string<'a>(&self, pid: i32, addr: u64, buf: &'a mut [u8]) -> SysResult<&'a [u8]> {
        self.read_bytes(pid, addr, buf)?;
        let len = buf
            .iter()
            .position(|&b| b == 0)
            .ok_or(SysError(Errno::RANGE))?;
        Ok(&buf[..len])
    }

    /// Write bytes followed by a NUL terminator.
    fn write_string(&self, pid: i32, addr: u64, src: &[u8]) -> SysResult<()> {
        self.write_bytes(pid, addr, src)?;
        self.write_bytes(pid, addr + src.len() as u64, &[0])
    }
}

/// Typed POD read/write helpers, available on any `GuestMem` (including
/// `dyn GuestMem`) without affecting object safety.
pub trait GuestMemExt: GuestMem {
    /// Read a POD value.
    fn read_val<T: Copy>(&self, pid: i32, addr: u64) -> SysResult<T> {
        let mut val = std::mem::MaybeUninit::<T>::uninit();
        // SAFETY: we fill exactly size_of::<T> bytes before assuming init.
        let buf = unsafe {
            std::slice::from_raw_parts_mut(val.as_mut_ptr() as *mut u8, std::mem::size_of::<T>())
        };
        self.read_bytes(pid, addr, buf)?;
        // SAFETY: buf was fully written by read_bytes on success.
        Ok(unsafe { val.assume_init() })
    }

    /// Write a POD value.
    fn write_val<T: Copy>(&self, pid: i32, addr: u64, val: &T) -> SysResult<()> {
        // SAFETY: T is Copy/POD; we read exactly size_of::<T> bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(val as *const T as *const u8, std::mem::size_of::<T>())
        };
        self.write_bytes(pid, addr, bytes)
    }
}

impl<M: GuestMem + ?Sized> GuestMemExt for M {}

fn from_nix(e: nix::errno::Errno) -> SysError {
    SysError(Errno::from_raw(e as i32).unwrap_or(Errno::IO))
}

/// Production bridge backed by process_vm_readv/writev.
pub struct RealGuestMem;

impl GuestMem for RealGuestMem {
    fn read_bytes(&self, pid: i32, addr: u64, buf: &mut [u8]) -> SysResult<()> {
        use nix::sys::uio::{process_vm_readv, RemoteIoVec};
        use nix::unistd::Pid;
        use std::io::IoSliceMut;

        let len = buf.len();
        let mut local = [IoSliceMut::new(buf)];
        let remote = [RemoteIoVec {
            base: addr as usize,
            len,
        }];
        process_vm_readv(Pid::from_raw(pid), &mut local, &remote).map_err(from_nix)?;
        Ok(())
    }

    fn write_bytes(&self, pid: i32, addr: u64, data: &[u8]) -> SysResult<()> {
        use nix::sys::uio::{process_vm_writev, RemoteIoVec};
        use nix::unistd::Pid;
        use std::io::IoSlice;

        let remote = [RemoteIoVec {
            base: addr as usize,
            len: data.len(),
        }];
        let local = [IoSlice::new(data)];
        process_vm_writev(Pid::from_raw(pid), &local, &remote).map_err(from_nix)?;
        Ok(())
    }
}

/// Test bridge: guest addresses are pointers in this process.
pub struct LocalMem;

impl GuestMem for LocalMem {
    fn read_bytes(&self, _pid: i32, addr: u64, buf: &mut [u8]) -> SysResult<()> {
        // SAFETY: tests pass a valid local pointer as `addr`.
        unsafe {
            std::ptr::copy_nonoverlapping(addr as *const u8, buf.as_mut_ptr(), buf.len());
        }
        Ok(())
    }

    fn write_bytes(&self, _pid: i32, addr: u64, data: &[u8]) -> SysResult<()> {
        // SAFETY: tests pass a valid local pointer as `addr`.
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), addr as *mut u8, data.len());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_mem_read_write_roundtrip() {
        let mem = LocalMem;
        let mut src = *b"hello\0world";
        let addr = src.as_mut_ptr() as u64;

        let mut buf = [0u8; 11];
        mem.read_bytes(0, addr, &mut buf).unwrap();
        assert_eq!(&buf, b"hello\0world");

        let mut sbuf = [0u8; 11];
        let s = mem.read_string(0, addr, &mut sbuf).unwrap();
        assert_eq!(s, b"hello");

        mem.write_bytes(0, addr, b"HELLO").unwrap();
        assert_eq!(&src[..5], b"HELLO");
    }

    #[test]
    fn local_mem_val_roundtrip() {
        let mem = LocalMem;
        let mut cell: u64 = 0;
        let addr = &mut cell as *mut u64 as u64;
        mem.write_val(0, addr, &0x1122334455667788u64).unwrap();
        let got: u64 = mem.read_val(0, addr).unwrap();
        assert_eq!(got, 0x1122334455667788);
    }
}
