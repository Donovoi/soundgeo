//! Bounded helper processes. Cancellation also stops download/merge descendants.
use crate::Control;
use anyhow::{Context, Result, bail};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

struct Tree {
    child: Child,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}
impl Tree {
    fn spawn(command: &mut Command) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = command.spawn().context("Could not start bundled helper; install the current SoundGeo release or place the tool on PATH")?;
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
            let mut child = child;
            // Each helper gets its own nested job. Closing it kills every descendant.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let assigned = !job.is_null()
                && unsafe {
                    SetInformationJobObject(
                        job,
                        JobObjectExtendedLimitInformation,
                        &info as *const _ as *const _,
                        std::mem::size_of_val(&info) as u32,
                    ) != 0
                        && AssignProcessToJobObject(job, child.as_raw_handle()) != 0
                };
            if !assigned {
                let error = std::io::Error::last_os_error();
                let _ = child.kill();
                let _ = child.wait();
                if !job.is_null() {
                    unsafe {
                        CloseHandle(job);
                    }
                }
                return Err(error).context("Could not contain downloader child processes");
            }
            Ok(Self { child, job })
        }
        #[cfg(not(windows))]
        Ok(Self { child })
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.job);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(crate) fn run(
    command: &mut Command,
    stdout: &Path,
    stderr: &Path,
    control: &Control,
    timeout: Duration,
    monitor: impl Fn() -> Result<()>,
) -> Result<ExitStatus> {
    control.check()?;
    command
        .stdin(Stdio::null())
        .stdout(File::create(stdout)?)
        .stderr(File::create(stderr)?);
    let mut process = Tree::spawn(command)?;
    let start = Instant::now();
    loop {
        control.check()?;
        if start.elapsed() > timeout {
            bail!("Download helper exceeded its time limit; partial files were retained");
        }
        if [stdout, stderr]
            .iter()
            .any(|p| p.metadata().is_ok_and(|m| m.len() > 32 * 1024 * 1024))
        {
            bail!("Download helper output exceeded 32 MiB");
        }
        monitor()?;
        if let Some(status) = process.child.try_wait()? {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
pub(crate) fn tail(path: &Path) -> String {
    let read = || -> std::io::Result<String> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(4000)))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    read().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_stops_descendant_writes() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("should-not-exist");
        let ready = dir.path().join("child-started");
        let control = Control::default();
        let cancel = control.clone();
        let ready_check = ready.clone();
        let thread = std::thread::spawn(move || {
            for _ in 0..100 {
                if ready_check.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let started = ready_check.exists();
            std::thread::sleep(Duration::from_millis(100));
            cancel.stop();
            started
        });
        #[cfg(unix)]
        let mut cmd = Command::new("sh");
        #[cfg(unix)]
        cmd.arg("-c")
            .arg("(touch \"$2\"; sleep 1.2; touch \"$1\") & wait")
            .arg("sh")
            .arg(&marker)
            .arg(&ready);
        #[cfg(windows)]
        let mut cmd = {
            let powershell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let child = dir.path().join("child.ps1");
            let parent = dir.path().join("parent.ps1");
            std::fs::write(&child, "[IO.File]::WriteAllText($args[1], 'ready')\nStart-Sleep -Milliseconds 1200\n[IO.File]::WriteAllText($args[0], 'unexpected')\n").unwrap();
            std::fs::write(&parent, "$p = Start-Process -WindowStyle Hidden -FilePath $args[0] -ArgumentList @('-NoProfile', '-File', ('\"{0}\"' -f $args[1]), ('\"{0}\"' -f $args[2]), ('\"{0}\"' -f $args[3])) -PassThru\n$p.WaitForExit()\n").unwrap();
            let mut command = crate::audio::quiet_command(&powershell);
            command
                .args(["-NoProfile", "-File"])
                .arg(parent)
                .arg(powershell)
                .arg(child)
                .arg(&marker)
                .arg(&ready);
            command
        };
        let result = run(
            &mut cmd,
            &dir.path().join("out"),
            &dir.path().join("err"),
            &control,
            Duration::from_secs(20),
            || Ok(()),
        );
        assert!(result.unwrap_err().to_string().contains("Cancelled"));
        assert!(thread.join().unwrap(), "Child did not start");
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!marker.exists());
    }
}
