use std::{
    ffi::OsString,
    io::Error,
    mem::{size_of, MaybeUninit},
    os::windows::prelude::*,
    process::exit,
    ptr::null_mut,
};

use winapi::{
    shared::minwindef::{FILETIME, TRUE},
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

        // 'cb' has to be set
        let mut si = MaybeUninit::<STARTUPINFOW>::zeroed().assume_init();
        si.cb = size_of::<STARTUPINFOW>() as u32;

        let mut pi = MaybeUninit::uninit();
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
            pi.as_mut_ptr(),
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

        Self {
            info: pi.assume_init(),
        }
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
        let mut limit = MaybeUninit::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>::uninit();
        let res = QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            limit.as_mut_ptr() as *mut _,
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

        limit.assume_init()
    }

    unsafe fn get_times(&self) -> Option<Times> {
        let handle = self.info.hProcess;

        let mut created = MaybeUninit::uninit();
        let mut killed = MaybeUninit::uninit();
        let mut sys = MaybeUninit::uninit();
        let mut user = MaybeUninit::uninit();

        if GetProcessTimes(
            handle,
            created.as_mut_ptr(),
            killed.as_mut_ptr(),
            sys.as_mut_ptr(),
            user.as_mut_ptr(),
        ) != TRUE
        {
            eprintln!("failed to get process times: {}", Error::last_os_error());
            return None;
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

        Some(Times {
            real: killed.assume_init().as_fractional_time()
                - created.assume_init().as_fractional_time(),
            user: user.assume_init().as_fractional_time(),
            sys: sys.assume_init().as_fractional_time(),
        })
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
        if let Some(times) = times {
            eprintln!("{}", times);
        }
        exit(pi.close())
    }
}
