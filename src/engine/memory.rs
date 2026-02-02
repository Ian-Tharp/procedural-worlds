//! Process Memory Tracking
//!
//! Cross-platform utilities for querying actual process memory usage
//! at runtime. Uses platform-specific OS APIs with zero external dependencies.
//!
//! - **Windows:** `K32GetProcessMemoryInfo` (kernel32)
//! - **Linux:** `/proc/self/statm`
//! - **macOS:** `mach_task_basic_info`
//! - **Other:** Returns `None`

/// Process memory snapshot.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessMemory {
    /// Resident Set Size (physical memory in use) in bytes.
    pub rss_bytes: usize,
    /// Peak RSS (high-water mark) in bytes, if available.
    pub peak_rss_bytes: Option<usize>,
}

/// Query current process memory from the operating system.
///
/// Returns `None` on unsupported platforms or if the query fails.
pub fn get_process_memory() -> Option<ProcessMemory> {
    platform::query()
}

/// Format a byte count into a human-readable string (e.g. "128.3 MB").
pub fn format_bytes(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

// ============================================================================
// Platform implementations
// ============================================================================

#[cfg(target_os = "windows")]
mod platform {
    use super::ProcessMemory;

    #[repr(C)]
    #[allow(non_snake_case)]
    struct ProcessMemoryCountersEx {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize,
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
        PrivateUsage: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            pmc: *mut ProcessMemoryCountersEx,
            cb: u32,
        ) -> i32;
    }

    pub fn query() -> Option<ProcessMemory> {
        unsafe {
            let mut pmc: ProcessMemoryCountersEx = std::mem::zeroed();
            pmc.cb = std::mem::size_of::<ProcessMemoryCountersEx>() as u32;

            let handle = GetCurrentProcess();
            if K32GetProcessMemoryInfo(handle, &mut pmc, pmc.cb) != 0 {
                Some(ProcessMemory {
                    rss_bytes: pmc.WorkingSetSize,
                    peak_rss_bytes: Some(pmc.PeakWorkingSetSize),
                })
            } else {
                None
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::ProcessMemory;

    pub fn query() -> Option<ProcessMemory> {
        // /proc/self/statm fields: size resident shared text lib data dt (in pages)
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let fields: Vec<&str> = statm.split_whitespace().collect();
        let resident_pages: usize = fields.get(1)?.parse().ok()?;
        let page_size = 4096_usize; // Typical Linux page size

        // Peak RSS from /proc/self/status VmHWM field
        let peak = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|status| {
                for line in status.lines() {
                    if line.starts_with("VmHWM:") {
                        let kb_str = line.split_whitespace().nth(1)?;
                        let kb: usize = kb_str.parse().ok()?;
                        return Some(kb * 1024);
                    }
                }
                None
            });

        Some(ProcessMemory {
            rss_bytes: resident_pages * page_size,
            peak_rss_bytes: peak,
        })
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::ProcessMemory;

    // mach/mach_types.h — MACH_TASK_BASIC_INFO
    const MACH_TASK_BASIC_INFO: i32 = 20;

    #[repr(C)]
    struct MachTaskBasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: [u32; 2],   // time_value_t
        system_time: [u32; 2], // time_value_t
        policy: i32,
        suspend_count: i32,
    }

    unsafe extern "C" {
        fn mach_task_self() -> u32;
        fn task_info(
            target_task: u32,
            flavor: i32,
            task_info_out: *mut MachTaskBasicInfo,
            task_info_count: *mut u32,
        ) -> i32;
    }

    pub fn query() -> Option<ProcessMemory> {
        unsafe {
            let mut info: MachTaskBasicInfo = std::mem::zeroed();
            let mut count =
                (std::mem::size_of::<MachTaskBasicInfo>() / std::mem::size_of::<u32>()) as u32;

            let kr = task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                &mut info,
                &mut count,
            );

            if kr == 0 {
                // KERN_SUCCESS
                Some(ProcessMemory {
                    rss_bytes: info.resident_size as usize,
                    peak_rss_bytes: Some(info.resident_size_max as usize),
                })
            } else {
                None
            }
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod platform {
    use super::ProcessMemory;

    pub fn query() -> Option<ProcessMemory> {
        None
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.00 GB");
        assert_eq!(format_bytes(134_217_728), "128.0 MB");
    }

    #[test]
    fn test_get_process_memory_returns_some() {
        // On supported platforms (Windows, Linux, macOS) this should succeed
        let mem = get_process_memory();
        if cfg!(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "macos"
        )) {
            let mem = mem.expect("Should return memory on supported platform");
            // Process should be using at least some memory
            assert!(mem.rss_bytes > 0, "RSS should be > 0, got {}", mem.rss_bytes);
        }
    }

    #[test]
    fn test_process_memory_default() {
        let mem = ProcessMemory::default();
        assert_eq!(mem.rss_bytes, 0);
        assert!(mem.peak_rss_bytes.is_none());
    }
}
