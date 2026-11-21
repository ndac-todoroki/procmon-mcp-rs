//! Windows calls the setup needs beyond the server crate's. All `unsafe` code lives here.

use std::fs::File;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{
    ERROR_CANCELLED, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0,
};
use windows::Win32::Foundation::{GlobalFree, HWND};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessId, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW, SW_HIDE};
use windows::core::{HSTRING, PCWSTR, w};

use crate::protocol::PipeId;

/// The window's end of the pipe, before the worker connects.
pub struct PipeServer {
    handle: OwnedHandle,
    path: String,
}

impl PipeServer {
    /// Creates the only instance of the pipe; fails if the name is already taken.
    pub fn create(id: &PipeId) -> io::Result<PipeServer> {
        let path = id.path();
        // SAFETY: plain call; the returned handle is checked and then owned.
        let handle = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(path.as_str()),
                PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                0,
                4096,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a valid handle that nothing else owns.
        let handle = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        Ok(PipeServer { handle, path })
    }

    /// Waits for `worker` to connect and returns the pipe to read its events from.
    ///
    /// Gives up if the worker exits first. Any other client (another program of the
    /// same user that found the name) is refused.
    pub fn accept(self, worker: Arc<OwnedHandle>) -> io::Result<File> {
        // ConnectNamedPipe has no timeout: if the worker dies before connecting, connect
        // to ourselves to release it, and let the process ID check below fail.
        let released = Arc::new(AtomicBool::new(false));
        {
            let worker = Arc::clone(&worker);
            let released = Arc::clone(&released);
            let path = self.path.clone();
            // Detached: it ends when the worker does.
            std::thread::spawn(move || {
                // SAFETY: the handle stays open while `worker` is held.
                let waited = unsafe { WaitForSingleObject(handle(&worker), INFINITE) };
                if waited == WAIT_OBJECT_0 {
                    released.store(true, Ordering::SeqCst);
                    let _ = std::fs::OpenOptions::new().write(true).open(path);
                }
            });
        }
        // SAFETY: the pipe handle is owned by `self`.
        match unsafe { ConnectNamedPipe(handle(&self.handle), None) } {
            Ok(()) => {}
            // Connected before we waited; NO_DATA: and already closed, its lines still buffered.
            Err(e)
                if e.code() == ERROR_PIPE_CONNECTED.to_hresult()
                    || e.code() == ERROR_NO_DATA.to_hresult() => {}
            Err(e) => return Err(e.into()),
        }
        let mut client = 0u32;
        // SAFETY: plain queries on owned handles.
        let (client, expected) = unsafe {
            GetNamedPipeClientProcessId(handle(&self.handle), &mut client)?;
            (client, GetProcessId(handle(&worker)))
        };
        if client != expected {
            return Err(io::Error::other(if released.load(Ordering::SeqCst) {
                "the elevated worker did not connect".to_owned()
            } else {
                format!("an unexpected process ({client}) connected to the setup's pipe")
            }));
        }
        Ok(File::from(self.handle))
    }
}

fn handle(owned: &OwnedHandle) -> HANDLE {
    HANDLE(owned.as_raw_handle())
}

/// Opens a process to wait for it and read its exit code.
pub fn open_process(pid: u32) -> io::Result<OwnedHandle> {
    // SAFETY: on success the handle is owned by the result.
    unsafe {
        let h = OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )?;
        Ok(OwnedHandle::from_raw_handle(h.0))
    }
}

/// Waits for the process to end and returns its exit code.
pub fn exit_code(process: &OwnedHandle) -> io::Result<u32> {
    let mut code = 0u32;
    // SAFETY: plain calls on an owned handle.
    unsafe {
        WaitForSingleObject(handle(process), INFINITE);
        GetExitCodeProcess(handle(process), &mut code)?;
    }
    Ok(code)
}

/// Why the elevated worker did not start.
#[derive(Debug)]
pub enum LaunchError {
    /// The UAC prompt was declined.
    Declined,
    Other(io::Error),
}

/// Starts this executable elevated with `args`, through the UAC prompt, owned by `parent`.
/// Arguments must not contain spaces or quotes; the worker's never do.
pub fn launch_elevated(parent: isize, args: &[String]) -> Result<OwnedHandle, LaunchError> {
    debug_assert!(args.iter().all(|a| !a.contains([' ', '"'])));
    let exe = std::env::current_exe().map_err(LaunchError::Other)?;
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(args.join(" "));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        hwnd: HWND(parent as *mut _),
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: COM is initialized for the shell on this thread, as ShellExecuteEx asks;
    // the strings outlive the call, and the returned process handle is owned below.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        match ShellExecuteExW(&mut info) {
            Ok(()) if !info.hProcess.is_invalid() => {
                Ok(OwnedHandle::from_raw_handle(info.hProcess.0))
            }
            Ok(()) => Err(LaunchError::Other(io::Error::other(
                "no process was started",
            ))),
            Err(e) if e.code() == ERROR_CANCELLED.to_hresult() => Err(LaunchError::Declined),
            Err(e) => Err(LaunchError::Other(e.into())),
        }
    }
}

/// The user's Windows display language.
pub fn ui_lang_id() -> u16 {
    // SAFETY: plain call without arguments.
    unsafe { GetUserDefaultUILanguage() }
}

/// Puts `text` on the clipboard.
pub fn copy_to_clipboard(text: &str) -> io::Result<()> {
    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the clipboard owns the memory once SetClipboardData succeeds; until then it
    // is freed here. The copy stays within the allocation of `wide.len()` units.
    unsafe {
        OpenClipboard(None)?;
        let result = (|| {
            EmptyClipboard()?;
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
            let dest = GlobalLock(mem).cast::<u16>();
            if dest.is_null() {
                let _ = GlobalFree(Some(mem));
                return Err(windows::core::Error::from_thread());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dest, wide.len());
            let _ = GlobalUnlock(mem);
            if let Err(e) = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))) {
                let _ = GlobalFree(Some(mem));
                return Err(e);
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        result.map_err(io::Error::from)
    }
}

/// A plain message box, for when WinUI itself cannot start.
pub fn message_box(title: &str, text: &str) {
    // SAFETY: plain call with NUL-terminated strings.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            &HSTRING::from(title),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    fn current_process() -> OwnedHandle {
        open_process(std::process::id()).unwrap()
    }

    #[test]
    fn accepts_the_expected_process_and_reads_its_lines() {
        let id = PipeId::new().unwrap();
        let server = PipeServer::create(&id).unwrap();
        let path = id.path();
        let writer = std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new().write(true).open(path).unwrap();
            pipe.write_all(b"step 0\nfinished\n").unwrap();
        });
        let pipe = server.accept(Arc::new(current_process())).unwrap();
        let lines: Vec<String> = BufReader::new(pipe).lines().map(Result::unwrap).collect();
        writer.join().unwrap();
        assert_eq!(lines, ["step 0", "finished"]);
    }

    #[test]
    fn a_second_server_with_the_same_name_is_refused() {
        let id = PipeId::new().unwrap();
        let _first = PipeServer::create(&id).unwrap();
        assert!(PipeServer::create(&id).is_err());
    }

    #[test]
    fn gives_up_when_the_worker_exits_without_connecting() {
        let id = PipeId::new().unwrap();
        let server = PipeServer::create(&id).unwrap();
        let child = std::process::Command::new("cmd.exe")
            .args(["/c", "exit", "3"])
            .spawn()
            .unwrap();
        let process = Arc::new(open_process(child.id()).unwrap());
        let err = server.accept(Arc::clone(&process)).unwrap_err();
        assert!(err.to_string().contains("did not connect"), "{err}");
        assert_eq!(exit_code(&process).unwrap(), 3);
    }

    #[test]
    fn refuses_a_client_other_than_the_worker() {
        let id = PipeId::new().unwrap();
        let server = PipeServer::create(&id).unwrap();
        let path = id.path();
        // The "worker" is a child that never connects; this process connects instead.
        let mut child = std::process::Command::new("cmd.exe")
            .args(["/c", "ping", "-n", "30", "127.0.0.1", ">nul"])
            .spawn()
            .unwrap();
        let process = Arc::new(open_process(child.id()).unwrap());
        let intruder = std::thread::spawn(move || {
            let _pipe = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        });
        let result = server.accept(process);
        let _ = child.kill();
        let _ = child.wait();
        intruder.join().unwrap();
        let err = result.unwrap_err();
        assert!(err.to_string().contains("unexpected"), "{err}");
    }
}
