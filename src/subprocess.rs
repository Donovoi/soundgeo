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

    fn fixture_command(role: &str, dir: &Path) -> Command {
        let mut command = crate::audio::quiet_command(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "subprocess::tests::cancellation_stops_descendant_writes",
                "--nocapture",
            ])
            .env("SOUNDGEO_SUBPROCESS_TEST_ROLE", role)
            .env("SOUNDGEO_SUBPROCESS_TEST_DIR", dir);
        command
    }

    fn wait_for(path: &Path, timeout: Duration) -> bool {
        let start = Instant::now();
        while !path.exists() && start.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(20));
        }
        path.exists()
    }

    #[test]
    fn cancellation_stops_descendant_writes() {
        // Re-enter this exact test as a native parent/child fixture. This avoids
        // shell startup, execution-policy and argument-quoting dependencies.
        if let Some(role) = std::env::var_os("SOUNDGEO_SUBPROCESS_TEST_ROLE") {
            let dir =
                std::path::PathBuf::from(std::env::var_os("SOUNDGEO_SUBPROCESS_TEST_DIR").unwrap());
            if role == "parent" {
                assert!(fixture_command("child", &dir).status().unwrap().success());
            } else {
                assert_eq!(role, "child");
                std::fs::write(dir.join("child-started"), "ready").unwrap();
                assert!(wait_for(&dir.join("allow-write"), Duration::from_secs(60)));
                std::fs::write(dir.join("should-not-exist"), "unexpected").unwrap();
            }
            return;
        }

        // First prove the fixture writes when allowed to finish normally.
        let normal = tempfile::tempdir().unwrap();
        std::fs::write(normal.path().join("allow-write"), "go").unwrap();
        let status = run(
            &mut fixture_command("parent", normal.path()),
            &normal.path().join("out"),
            &normal.path().join("err"),
            &Control::default(),
            Duration::from_secs(30),
            || Ok(()),
        );
        assert!(
            status.as_ref().is_ok_and(ExitStatus::success),
            "Native fixture failed: {status:?}; stdout: {}; stderr: {}",
            tail(&normal.path().join("out")),
            tail(&normal.path().join("err")),
        );
        assert!(normal.path().join("should-not-exist").exists());

        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("should-not-exist");
        let ready = dir.path().join("child-started");
        let control = Control::default();
        let cancel = control.clone();
        let ready_check = ready.clone();
        let thread = std::thread::spawn(move || {
            let started = wait_for(&ready_check, Duration::from_secs(30));
            cancel.stop();
            started
        });
        let result = run(
            &mut fixture_command("parent", dir.path()),
            &dir.path().join("out"),
            &dir.path().join("err"),
            &control,
            Duration::from_secs(45),
            || Ok(()),
        );
        let started = thread.join().unwrap();
        assert!(
            started,
            "Native child did not start: {result:?}; stdout: {}; stderr: {}",
            tail(&dir.path().join("out")),
            tail(&dir.path().join("err")),
        );
        assert!(result.unwrap_err().to_string().contains("Cancelled"));
        // Release the child's write only after cancellation has returned. A
        // surviving descendant would now create the marker, independent of how
        // long startup or scheduling took before cancellation.
        std::fs::write(dir.path().join("allow-write"), "go").unwrap();
        assert!(
            !wait_for(&marker, Duration::from_secs(2)),
            "A descendant kept running after cancellation"
        );
    }
}
