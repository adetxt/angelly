use std::path::PathBuf;

/// A process running in the terminal's foreground process group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    /// Executable name (e.g. `vim`, `zsh`).
    pub name: String,
    pub cwd: Option<PathBuf>,
}

#[cfg(target_os = "macos")]
pub(crate) fn lookup(pid: u32) -> Option<ProcessInfo> {
    use std::{ffi::CStr, mem::MaybeUninit};

    let mut name = [0u8; 256];
    let n = unsafe { libc::proc_name(pid as i32, name.as_mut_ptr() as *mut _, name.len() as u32) };
    if n <= 0 {
        return None;
    }
    let name = String::from_utf8_lossy(&name[..n as usize]).into_owned();

    let mut info = MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
    let got = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr() as *mut _,
            size,
        )
    };
    let cwd = (got == size).then(|| {
        let info = unsafe { info.assume_init() };
        let path =
            unsafe { CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr() as *const libc::c_char) };
        PathBuf::from(path.to_string_lossy().into_owned())
    });
    Some(ProcessInfo { pid, name, cwd })
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn lookup(pid: u32) -> Option<ProcessInfo> {
    let name = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok();
    Some(ProcessInfo {
        pid,
        name: name.trim_end().to_owned(),
        cwd,
    })
}

#[cfg(windows)]
pub(crate) use windows::{foreground_descendant, lookup};

/// Windows has no foreground process group on a ConPTY. The shell's deepest
/// descendant that is not console plumbing stands in for it.
#[cfg(windows)]
mod windows {
    use std::{ffi::c_void, mem, path::PathBuf};

    use windows_sys::{
        Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation},
        Win32::{
            Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
            System::{
                Diagnostics::{
                    Debug::ReadProcessMemory,
                    ToolHelp::{
                        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                        TH32CS_SNAPPROCESS,
                    },
                },
                Threading::{
                    OpenProcess, PROCESS_BASIC_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
                    PROCESS_VM_READ,
                },
            },
        },
    };

    use super::ProcessInfo;

    struct Entry {
        pid: u32,
        parent: u32,
        name: String,
    }

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    fn snapshot() -> Vec<Entry> {
        let mut entries = Vec::new();
        let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if handle == INVALID_HANDLE_VALUE {
            return entries;
        }
        let handle = Handle(handle);
        let mut entry: PROCESSENTRY32W = unsafe { mem::zeroed() };
        entry.dwSize = mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = unsafe { Process32FirstW(handle.0, &mut entry) } != 0;
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            entries.push(Entry {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..len]),
            });
            more = unsafe { Process32NextW(handle.0, &mut entry) } != 0;
        }
        entries
    }

    fn is_console_helper(name: &str) -> bool {
        ["conhost.exe", "openconsole.exe"].iter().any(|helper| name.eq_ignore_ascii_case(helper))
    }

    pub(crate) fn foreground_descendant(root: u32) -> Option<u32> {
        let entries = snapshot();
        let mut best = (0usize, root);
        let mut stack = vec![(root, 0usize)];
        let mut seen = vec![root];
        while let Some((pid, depth)) = stack.pop() {
            for child in entries.iter().filter(|e| e.parent == pid && !is_console_helper(&e.name)) {
                if seen.contains(&child.pid) {
                    continue;
                }
                seen.push(child.pid);
                if (depth + 1, child.pid) > best {
                    best = (depth + 1, child.pid);
                }
                stack.push((child.pid, depth + 1));
            }
        }
        Some(best.1)
    }

    pub(crate) fn lookup(pid: u32) -> Option<ProcessInfo> {
        let entry = snapshot().into_iter().find(|e| e.pid == pid)?;
        let name = match entry.name.rsplit_once('.') {
            Some((stem, ext)) if ext.eq_ignore_ascii_case("exe") => stem.to_owned(),
            _ => entry.name,
        };
        Some(ProcessInfo { pid, name, cwd: working_directory(pid) })
    }

    unsafe fn read<T: Copy>(process: HANDLE, address: usize) -> Option<T> {
        let mut value = mem::MaybeUninit::<T>::uninit();
        let ok = ReadProcessMemory(
            process,
            address as *const c_void,
            value.as_mut_ptr().cast(),
            mem::size_of::<T>(),
            std::ptr::null_mut(),
        );
        (ok != 0).then(|| value.assume_init())
    }

    /// Reads the current directory out of the target's `RTL_USER_PROCESS_PARAMETERS`.
    /// The offsets are those of 64-bit processes; a 32-bit target yields `None`.
    #[cfg(target_pointer_width = "64")]
    fn working_directory(pid: u32) -> Option<PathBuf> {
        const PARAMETERS_OFFSET_IN_PEB: usize = 0x20;
        const CURRENT_DIRECTORY_OFFSET: usize = 0x38;

        #[derive(Clone, Copy)]
        #[repr(C)]
        struct UnicodeString {
            length: u16,
            maximum_length: u16,
            buffer: usize,
        }

        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if process.is_null() {
            return None;
        }
        let process = Handle(process);
        let mut info: PROCESS_BASIC_INFORMATION = unsafe { mem::zeroed() };
        let status = unsafe {
            NtQueryInformationProcess(
                process.0,
                ProcessBasicInformation,
                (&mut info as *mut PROCESS_BASIC_INFORMATION).cast(),
                mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if status < 0 || info.PebBaseAddress.is_null() {
            return None;
        }
        unsafe {
            let parameters: usize = read(process.0, info.PebBaseAddress as usize + PARAMETERS_OFFSET_IN_PEB)?;
            let path: UnicodeString = read(process.0, parameters + CURRENT_DIRECTORY_OFFSET)?;
            if path.buffer == 0 || path.length == 0 || path.length > 32_768 {
                return None;
            }
            let mut wide = vec![0u16; path.length as usize / 2];
            let ok = ReadProcessMemory(
                process.0,
                path.buffer as *const c_void,
                wide.as_mut_ptr().cast(),
                path.length as usize,
                std::ptr::null_mut(),
            );
            if ok == 0 {
                return None;
            }
            let text = String::from_utf16_lossy(&wide);
            let trimmed = if text.len() > 3 { text.trim_end_matches('\\') } else { text.as_str() };
            Some(PathBuf::from(trimmed))
        }
    }

    #[cfg(not(target_pointer_width = "64"))]
    fn working_directory(_pid: u32) -> Option<PathBuf> {
        None
    }
}
