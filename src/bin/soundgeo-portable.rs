#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(windows)]
fn launch() -> anyhow::Result<i32> {
    use anyhow::Context;
    use std::{ffi::OsStr, os::windows::process::CommandExt, path::PathBuf, process::Command};
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let exe = std::env::current_exe()?;
    if args
        .first()
        .is_some_and(|a| a == OsStr::new("--portable-extract"))
    {
        anyhow::ensure!(
            args.len() == 2,
            "Use --portable-extract DIRECTORY (a new folder)"
        );
        let folder = soundgeo::portable::extract_to(&exe, &PathBuf::from(&args[1]))?;
        println!("{}", folder.display());
        return Ok(0);
    }
    let base = PathBuf::from(
        std::env::var_os("LOCALAPPDATA").context("Windows LOCALAPPDATA is unavailable")?,
    )
    .join("SoundGeo")
    .join("runtime");
    let folder = soundgeo::portable::prepare_cached(&exe, &base)?;
    let name = if args.is_empty() {
        "soundgeo.exe"
    } else {
        "soundgeo-cli.exe"
    };
    let status = Command::new(folder.join(name))
        .args(&args)
        .env("GALLIUM_DRIVER", "llvmpipe")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("LP_NUM_THREADS", "2")
        .creation_flags(0x08000000) // no extra console window; redirected handles still work
        .status()
        .context("Could not start the bundled SoundGeo application")?;
    Ok(status.code().unwrap_or(1))
}

fn main() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBoxW(
                window: *mut std::ffi::c_void,
                text: *const u16,
                caption: *const u16,
                flags: u32,
            ) -> i32;
        }
        let console = std::env::args_os().len() > 1;
        if console {
            unsafe {
                AttachConsole(u32::MAX);
            }
        }
        match launch() {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("{error:#}");
                if !console {
                    let message = format!("SoundGeo could not start.\n\n{error:#}\0")
                        .encode_utf16()
                        .collect::<Vec<_>>();
                    let title = "SoundGeo\0".encode_utf16().collect::<Vec<_>>();
                    unsafe {
                        MessageBoxW(std::ptr::null_mut(), message.as_ptr(), title.as_ptr(), 0x10);
                    }
                }
                std::process::exit(1);
            }
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!(
            "This launcher is packaged for Windows. Run soundgeo or soundgeo-cli on this platform."
        );
        std::process::exit(1);
    }
}
