use std::{
    ffi::OsString,
    io::Error,
    mem::{size_of, zeroed},
    os::windows::prelude::*,
    process::exit,
    ptr::null_mut,
};

use winapi::{
    shared::minwindef::{FILETIME, LPVOID, TRUE},
    um::{
        handleapi::CloseHandle,
        jobapi2::{AssignProcessToJobObject, CreateJobObjectW, QueryInformationJobObject},
        processthreadsapi::{
            CreateProcessW, GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, ResumeThread,
            PROCESS_INFORMATION, STARTUPINFOW,
        },
        synchapi::WaitForSingleObject,
        winbase::{CREATE_SUSPENDED, INFINITE},
        winnt::{JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION},
    },
};

struct ProcessInfo {
    info: PROCESS_INFORMATION,
}

impl ProcessInfo {
    unsafe fn create(command: OsString) -> Self {
        let wstr: Vec<u16> = command.as_os_str().encode_wide().collect();

        let mut si = zeroed::<STARTUPINFOW>();
        si.cb = size_of::<STARTUPINFOW>() as u32;

        let mut pi = zeroed();
        let res = CreateProcessW(
            null_mut(),
            wstr.as_ptr() as *mut _,
            null_mut(),
            null_mut(),
            TRUE,
            CREATE_SUSPENDED,
            null_mut(),
            null_mut(),
            &mut si,
            &mut pi,
        );

        if res != TRUE {
            eprintln!(
                "failed to spawn subprocess, error: {}",
                Error::last_os_error()
            );
            eprintln!(
                "you may need to use \"cmd /c {}\"",
                command.into_string().unwrap()
            );
            exit(1);
        }

        Self { info: pi }
    }

    unsafe fn spawn(&self) -> JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        let job = CreateJobObjectW(null_mut(), null_mut());
        if AssignProcessToJobObject(job, self.info.hProcess) != TRUE {
            eprintln!(
                "failed to AssignProcessToJobObject, error: {}",
                Error::last_os_error()
            );
            // not a fatal error
        }

        ResumeThread(self.info.hThread);
        CloseHandle(self.info.hThread);

        WaitForSingleObject(self.info.hProcess, INFINITE);
        let mut limit: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        let res = QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut limit as *mut _ as _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            null_mut(),
        );

        if res != TRUE {
            eprintln!(
                "failed to QueryInformationJobObject, error: {}",
                Error::last_os_error()
            );
            // not a fatal error
        }

        CloseHandle(job);

        limit
    }

    unsafe fn get_times(&self) -> Times {
        let handle = self.info.hProcess;

        let mut created = zeroed();
        let mut killed = zeroed();
        let mut sys = zeroed();
        let mut user = zeroed();

        if GetProcessTimes(handle, &mut created, &mut killed, &mut sys, &mut user) != TRUE {
            eprintln!("failed to get process times: {}", Error::last_os_error());
        }

        if GetCurrentProcess() == handle {
            killed = created
        }

        trait AsFractionalTime {
            fn as_fractional_time(&self) -> f64;
        }

        impl AsFractionalTime for FILETIME {
            fn as_fractional_time(&self) -> f64 {
                let low = self.dwLowDateTime as usize;
                let high = self.dwHighDateTime as usize;
                let time = (low | (high << 32)) as f64;
                time * 0.000_000_1
            }
        }

        Times {
            real: killed.as_fractional_time() - created.as_fractional_time(),
            user: user.as_fractional_time(),
            sys: sys.as_fractional_time(),
        }
    }

    unsafe fn close(self) -> i32 {
        let mut exit_code = 0;
        GetExitCodeProcess(self.info.hProcess, &mut exit_code);
        CloseHandle(self.info.hProcess);
        exit_code as _
    }
}

struct Times {
    real: f64,
    user: f64,
    sys: f64,
}

impl std::fmt::Display for Times {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "real\t{:.3}s", self.real)?;
        writeln!(f, "user\t{:.3}s", self.user)?;
        writeln!(f, "sys\t{:.3}s", self.sys)
    }
}

fn main() {
    let mut command = std::env::args_os()
        .skip(1)
        .fold(OsString::with_capacity(256), |mut s, a| {
            if !s.is_empty() {
                s.push(" ");
            }
            s.push(a);
            s
        });
    command.push("\0");

    unsafe {
        let pi = ProcessInfo::create(command);
        let limit = pi.spawn();
        let times = pi.get_times();
        eprintln!(
            "\npeak\t{:.2}MiB",
            (limit.PeakProcessMemoryUsed as f64) / 1e6
        );
        eprintln!("{}", times);
        exit(pi.close())
    }
}
