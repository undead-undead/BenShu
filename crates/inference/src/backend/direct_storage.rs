//! DirectStorage.rs — Windows-native NVMe Accelerator for BenShu
//! Refined Phase 18.1: Memory-Safe, Batched, and Robust Direct-to-VRAM/RAM streaming.

use crate::backend::{InferenceError, Result};
use std::alloc::{self, Layout};
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};

#[cfg(target_os = "windows")]
mod windows_native {
    use crate::backend::InferenceError;
    use std::os::windows::io::RawHandle;
    use std::ptr::null_mut;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *mut libc::c_void,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: RawHandle,
        ) -> RawHandle;

        pub fn ReadFile(
            hFile: RawHandle,
            lpBuffer: *mut libc::c_void,
            nNumberOfBytesToRead: u32,
            lpNumberOfBytesRead: *mut u32,
            lpOverlapped: *mut OVERLAPPED,
        ) -> i32;

        pub fn CloseHandle(hObject: RawHandle) -> i32;
        pub fn GetOverlappedResult(
            hFile: RawHandle,
            lpOverlapped: *mut OVERLAPPED,
            lpNumberOfBytesTransferred: *mut u32,
            bWait: i32,
        ) -> i32;
    }

    #[repr(C)]
    pub struct OVERLAPPED {
        pub internal: usize,
        pub internal_high: usize,
        pub offset: u32,
        pub offset_high: u32,
        pub h_event: RawHandle,
    }

    pub const GENERIC_READ: u32 = 0x80000000;
    pub const FILE_SHARE_READ: u32 = 1;
    pub const OPEN_EXISTING: u32 = 3;
    pub const FILE_FLAG_NO_BUFFERING: u32 = 0x20000000;
    pub const FILE_FLAG_OVERLAPPED: u32 = 0x40000000;
    pub const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;

    pub struct DirectLoader {
        handle: RawHandle,
        pub file_size: u64,
    }

    impl DirectLoader {
        pub fn open(path: &std::path::Path) -> std::io::Result<Self> {
            use std::os::windows::ffi::OsStrExt;
            let mut wide_path: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide_path.push(0);

            let handle = unsafe {
                CreateFileW(
                    wide_path.as_ptr(),
                    GENERIC_READ,
                    FILE_SHARE_READ,
                    null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_NO_BUFFERING | FILE_FLAG_OVERLAPPED,
                    null_mut(),
                )
            };

            if handle == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }

            let file_size = std::fs::metadata(path)?.len();
            Ok(Self { handle, file_size })
        }

        /// Perform IO pump with verified completion.
        pub fn pump_to_vram_blocking(
            &self,
            offset: u64,
            size: usize,
            buffer: *mut u8,
        ) -> super::Result<usize> {
            let mut overlapped = OVERLAPPED {
                internal: 0,
                internal_high: 0,
                offset: (offset & 0xFFFFFFFF) as u32,
                offset_high: (offset >> 32) as u32,
                h_event: null_mut(),
            };

            let mut transferred = 0;
            let res = unsafe {
                ReadFile(
                    self.handle,
                    buffer as *mut _,
                    size as u32,
                    null_mut(),
                    &mut overlapped,
                )
            };

            // res == 0 means pending or actual error
            if res == 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(997) {
                    // 997 = ERROR_IO_PENDING
                    return Err(InferenceError::Execution(
                        format!("Hardware IO Failed: {}", err),
                        "direct_storage".to_string(),
                    ));
                }

                // Explicitly wait for async result (This is synchronous on the background thread)
                let ok = unsafe {
                    GetOverlappedResult(self.handle, &mut overlapped, &mut transferred, 1)
                };
                if ok == 0 {
                    return Err(InferenceError::Execution(
                        format!(
                            "Async IO completion failed: {}",
                            std::io::Error::last_os_error()
                        ),
                        "direct_storage".to_string(),
                    ));
                }
            } else {
                // Immediate completion
                unsafe {
                    GetOverlappedResult(self.handle, &mut overlapped, &mut transferred, 0);
                }
            }

            Ok(transferred as usize)
        }
    }

    impl Drop for DirectLoader {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

pub struct IoMetrics {
    pub duration_ms: f64,
    pub throughput_gbps: f64,
}

pub struct DirectStorageManager {
    batch_size: usize,
}

impl DirectStorageManager {
    pub fn new() -> Self {
        Self {
            // 64MB batches for optimal NVMe pressure vs responsiveness
            batch_size: 64 * 1024 * 1024,
        }
    }

    /// Optimized loading with memory safety guards and batching
    pub fn load_massive_sync(&self, path: &Path) -> Result<(Vec<u8>, IoMetrics)> {
        let start = Instant::now();

        #[cfg(target_os = "windows")]
        {
            let metadata =
                std::fs::metadata(path).map_err(|e| InferenceError::LoadFailed(e.to_string()))?;
            let size = metadata.len() as usize;
            if size == 0 {
                return Ok((
                    Vec::new(),
                    IoMetrics {
                        duration_ms: 0.0,
                        throughput_gbps: 0.0,
                    },
                ));
            }

            let align = 4096;
            if !is_direct_io_compatible(size, align) {
                warn!(
                    "DirectStorage alignment requirements not met for {} (size={} bytes). Falling back to StdIO.",
                    path.display(),
                    size
                );
                return self.fallback_load(path, start);
            }

            let loader = match windows_native::DirectLoader::open(path) {
                Ok(l) => l,
                Err(e) => {
                    warn!("DirectStorage open failed, falling back to StdIO: {}", e);
                    return self.fallback_load(path, start);
                }
            };

            // 4KB alignment for NO_BUFFERING
            let aligned_size = (size + align - 1) & !(align - 1);
            let layout = Layout::from_size_align(aligned_size, align)
                .map_err(|e| InferenceError::LoadFailed(format!("Layout error: {}", e)))?;

            let ptr = unsafe { alloc::alloc_zeroed(layout) };
            if ptr.is_null() {
                return Err(InferenceError::LoadFailed(
                    "Out of memory for aligned buffer".into(),
                ));
            }

            let result = (|| {
                let mut current_offset = 0u64;
                while current_offset < loader.file_size {
                    let chunk =
                        std::cmp::min(self.batch_size as u64, loader.file_size - current_offset)
                            as usize;
                    let target_ptr = unsafe { ptr.add(current_offset as usize) };

                    let transferred =
                        loader.pump_to_vram_blocking(current_offset, chunk, target_ptr)?;

                    if transferred == 0 {
                        return Err(InferenceError::Execution(
                            "Unexpected EOF during NVMe pump".into(),
                            "direct_storage".to_string(),
                        ));
                    }
                    current_offset += transferred as u64;
                }
                Ok(())
            })();

            if let Err(e) = result {
                unsafe {
                    alloc::dealloc(ptr, layout);
                }
                return Err(e);
            }

            // Success: hand over ownership to Vec
            let vec = unsafe { Vec::from_raw_parts(ptr, size, aligned_size) };
            let duration = start.elapsed();
            let metrics = IoMetrics {
                duration_ms: duration.as_secs_f64() * 1000.0,
                throughput_gbps: (size as f64 * 8.0) / (duration.as_secs_f64() * 1e9),
            };

            info!(
                "🚀 DirectStorage Load: {:.2} GB | {:.2} ms | {:.2} Gbps",
                size as f64 / 1e9,
                metrics.duration_ms,
                metrics.throughput_gbps
            );

            Ok((vec, metrics))
        }

        #[cfg(not(target_os = "windows"))]
        {
            self.fallback_load(path, start)
        }
    }

    fn fallback_load(&self, path: &Path, start: Instant) -> Result<(Vec<u8>, IoMetrics)> {
        let data = std::fs::read(path).map_err(|e| InferenceError::LoadFailed(e.to_string()))?;
        let duration = start.elapsed();
        let metrics = IoMetrics {
            duration_ms: duration.as_secs_f64() * 1000.0,
            throughput_gbps: (data.len() as f64 * 8.0) / (duration.as_secs_f64() * 1e9),
        };
        Ok((data, metrics))
    }
}

#[cfg(target_os = "windows")]
fn is_direct_io_compatible(file_size: usize, alignment: usize) -> bool {
    file_size > 0 && file_size % alignment == 0
}

#[cfg(not(target_os = "windows"))]
fn is_direct_io_compatible(_file_size: usize, _alignment: usize) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_direct_storage_safety() {
        let mut file = NamedTempFile::new().unwrap();
        let data = vec![0u8; 8192]; // 2 x 4KB pages
        file.write_all(&data).unwrap();

        let manager = DirectStorageManager::new();
        let (loaded, metrics) = manager.load_massive_sync(file.path()).unwrap();

        assert_eq!(loaded.len(), 8192);
        assert!(metrics.duration_ms >= 0.0);
    }
}
