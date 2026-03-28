//! Anonymous shared-memory buffer for screencopy frame data.
//!
//! Creates a memfd, ftruncates it to the frame size, mmaps it read-write,
//! then hands the fd to the compositor via wl_shm_pool.  The compositor
//! writes pixel data into the mapping and fires the `ready` event.

use std::{ptr, slice};

use anyhow::Context;
use rustix::{
    fd::OwnedFd,
    fs::{memfd_create, MemfdFlags, ftruncate},
    mm::{mmap, munmap, MapFlags, ProtFlags},
};

/// Owned SHM allocation: memfd + mmap.
///
/// The fd stays open while the `WlShmPool` is alive (compositor holds its own
/// mapping).  Pixel data is readable via `data()` once the compositor fires
/// `ready`.
pub struct ShmAlloc {
    pub fd:  OwnedFd,
    ptr:     *mut u8,
    pub len: usize,
}

// SAFETY: the raw pointer is a valid private mmap that we own exclusively.
unsafe impl Send for ShmAlloc {}
unsafe impl Sync for ShmAlloc {}

impl ShmAlloc {
    pub fn new(len: usize) -> anyhow::Result<Self> {
        if len == 0 {
            anyhow::bail!("ShmAlloc: zero-length buffer");
        }
        let fd = memfd_create("woven-screencopy", MemfdFlags::CLOEXEC)
        .context("memfd_create failed")?;
        ftruncate(&fd, len as u64).context("ftruncate failed")?;

        // SAFETY: fd is valid; len > 0; MAP_SHARED is correct for wl_shm.
        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                 len,
                 ProtFlags::READ | ProtFlags::WRITE,
                 MapFlags::SHARED,
                 &fd,
                 0,
            )
            .context("mmap failed")?
        } as *mut u8;

        Ok(Self { fd, ptr, len })
    }

    /// Read pixel bytes after the compositor fires `ready`.
    pub fn data(&self) -> &[u8] {
        // SAFETY: compositor has finished writing; len matches the allocation.
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for ShmAlloc {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            // SAFETY: ptr/len match the original mmap call.
            let _ = unsafe { munmap(self.ptr.cast(), self.len) };
        }
    }
}
