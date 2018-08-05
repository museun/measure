extern crate winapi;

use winapi::shared::{minwindef, ntdef};
use winapi::um::{errhandlingapi, handleapi, jobapi2, processthreadsapi, synchapi, winbase, winnt};

use std::ffi::OsString;
use std::os::windows::prelude::*;
use std::{env, fmt, mem, ptr};

fn main() {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let mut command = OsString::with_capacity(args.iter().map(|i| i.len()).sum());
    for s in &args {
        command.push(s);
        command.push(" ");
    }
    command.push("\0");
    let wstr = command.as_os_str().encode_wide().collect::<Vec<_>>();

    let mut si: processthreadsapi::STARTUPINFOW = unsafe { mem::zeroed() };
    si.cb = mem::size_of::<processthreadsapi::STARTUPINFOW>() as u32;
    let mut pi: processthreadsapi::PROCESS_INFORMATION = unsafe { mem::zeroed() };

    let res = unsafe {
        processthreadsapi::CreateProcessW(
            ptr::null_mut(),
            wstr.as_ptr() as *mut _,
            ptr::null_mut(),
            ptr::null_mut(),
            minwindef::TRUE,
            winbase::CREATE_SUSPENDED,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut si,
            &mut pi,
        )
    };
    if res != minwindef::TRUE {
        let (err, msg) = get_last_error();
        eprintln!("failed to spawn subprocess, error: ({}) {}", err, msg);
        eprintln!(
            "you may need to use \"cmd /c {}\"",
            command.into_string().unwrap()
        );
        ::std::process::exit(1)
    }

    let job = unsafe { jobapi2::CreateJobObjectW(ptr::null_mut(), ptr::null_mut()) };
    if unsafe { jobapi2::AssignProcessToJobObject(job, pi.hProcess) } != minwindef::TRUE {
        let (err, msg) = get_last_error();
        eprintln!(
            "failed to AssignProcessToJobObject, error: ({}) {}",
            err, msg
        );
    }

    unsafe { processthreadsapi::ResumeThread(pi.hThread) };
    unsafe { handleapi::CloseHandle(pi.hThread) };

    unsafe { synchapi::WaitForSingleObject(pi.hProcess, winbase::INFINITE) };
    let mut limit: winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    let res = unsafe {
        jobapi2::QueryInformationJobObject(
            job,
            winnt::JobObjectExtendedLimitInformation,
            &mut limit as *mut _ as minwindef::LPVOID,
            mem::size_of::<winnt::JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ptr::null_mut(),
        )
    };
    if res != minwindef::TRUE {
        let (err, msg) = get_last_error();
        eprintln!(
            "failed to QueryInformationJobObject, error: ({}) {}",
            err, msg
        );
    }
    unsafe { handleapi::CloseHandle(job) };

    let times = Times::new(pi.hProcess);
    let mut exit_code = 0;
    unsafe { processthreadsapi::GetExitCodeProcess(pi.hProcess, &mut exit_code) };
    unsafe { handleapi::CloseHandle(pi.hProcess) };

    println!(
        "\npeak\t{:.2}MiB",
        (limit.PeakProcessMemoryUsed as f64) / 1e6
    );

    println!("{}", times);
}

fn get_last_error() -> (u32, String) {
    let err = unsafe { errhandlingapi::GetLastError() };
    let mut msg = vec![0u16; 127]; // fixed length strings in the windows api
    unsafe {
        winbase::FormatMessageW(
            winbase::FORMAT_MESSAGE_FROM_SYSTEM | winbase::FORMAT_MESSAGE_IGNORE_INSERTS,
            ptr::null_mut(),
            err,
            u32::from(ntdef::LANG_SYSTEM_DEFAULT),
            msg.as_mut_ptr(),
            msg.len() as u32,
            ptr::null_mut(),
        );
    }
    let s = String::from_utf16_lossy(&msg);
    (err, s)
}

struct Times {
    real: f64,
    user: f64,
    sys: f64,
}

impl Times {
    fn new(hn: winnt::HANDLE) -> Self {
        use winapi::shared::minwindef::FILETIME;
        let (mut created, mut killed, mut sys, mut user) =
            unsafe { (mem::zeroed(), mem::zeroed(), mem::zeroed(), mem::zeroed()) };

        let res = unsafe {
            processthreadsapi::GetProcessTimes(hn, &mut created, &mut killed, &mut sys, &mut user)
        };
        if res != minwindef::TRUE {
            let (err, msg) = get_last_error();
            eprintln!("failed to get process times, error: ({}) {}", err, msg);
            ::std::process::exit(1)
        }
        if hn == unsafe { processthreadsapi::GetCurrentProcess() } {
            killed = created
        }

        let convert = |ft: &FILETIME| {
            let f = (ft.dwLowDateTime as usize) | ((ft.dwHighDateTime as usize) << 32);
            (f as f64) * 0.000_000_1
        };

        let real = convert(&killed) - convert(&created);
        Self {
            real,
            user: convert(&user),
            sys: convert(&sys),
        }
    }
}

impl fmt::Display for Times {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "real\t{:.3}s", self.real)?;
        writeln!(f, "user\t{:.3}s", self.user)?;
        writeln!(f, "sys\t{:.3}s", self.sys)
    }
}
