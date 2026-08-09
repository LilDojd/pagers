use pagers_core::{Cancellation, ops, output};
use std::os::fd::OwnedFd;

use crate::Error;
use crate::cli::LockInner;

pub(crate) enum ForkOutcome {
    Parent,
    Child(Option<OwnedFd>),
}

pub(crate) fn go_daemon(wait: bool) -> Result<ForkOutcome, Error> {
    let pipe = if wait {
        Some(nix::unistd::pipe()?)
    } else {
        None
    };

    match unsafe { nix::unistd::fork() }? {
        nix::unistd::ForkResult::Parent { child: _ } => {
            if let Some((read_fd, _)) = pipe {
                wait_for_child(read_fd)?;
            }
            Ok(ForkOutcome::Parent)
        }
        nix::unistd::ForkResult::Child => {
            nix::unistd::setsid()?;
            if let Some((_, write_fd)) = pipe {
                Ok(ForkOutcome::Child(Some(write_fd)))
            } else {
                redirect_stdio();
                Ok(ForkOutcome::Child(None))
            }
        }
    }
}

fn wait_for_child(read_fd: OwnedFd) -> Result<(), Error> {
    use std::io::Read;
    let mut file = std::fs::File::from(read_fd);
    let mut buf = [0u8; 1];
    match file.read(&mut buf) {
        Ok(1) if buf[0] == 0 => Ok(()),
        Ok(1) => Err(Error::DaemonExit(buf[0])),
        _ => Err(Error::DaemonShutdown),
    }
}

fn redirect_stdio() {
    use std::os::fd::FromRawFd;
    if let Ok(devnull) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
    {
        for raw in [0, 1, 2] {
            let mut fd = unsafe { OwnedFd::from_raw_fd(raw) };
            let _ = nix::unistd::dup2(&devnull, &mut fd);
            std::mem::forget(fd);
        }
    }
}

pub(crate) fn hold(
    stats: &ops::Stats,
    inner: &LockInner,
    cancellation: &Cancellation,
    mut notify_fd: Option<OwnedFd>,
) -> Result<(), Error> {
    if let Some(p) = &inner.pidfile
        && let Err(source) = fs_err::write(p, format!("{}\n", std::process::id()))
    {
        let error = Error::Core(pagers_core::Error::io(
            format!("pidfile {}", p.display()),
            source,
        ));
        if notify_fd.is_some() {
            eprintln!("{error}");
        }
        notify_and_redirect(notify_fd.take(), 1);
        return Err(error);
    }

    let page_size = *pagers_core::pagesize::PAGE_SIZE;
    let total = stats.total_pages.load(std::sync::atomic::Ordering::Relaxed);
    ::tracing::info!(
        "LOCKED {} pages ({})",
        total,
        output::pretty_size(total * page_size)
    );

    notify_and_redirect(notify_fd, 0);

    cancellation.wait();

    if let Some(p) = &inner.pidfile {
        let _ = fs_err::remove_file(p);
    }
    Ok(())
}

pub(crate) fn notify_and_redirect(notify_fd: Option<OwnedFd>, status: u8) {
    if let Some(fd) = notify_fd {
        use std::io::Write;
        let mut file = std::fs::File::from(fd);
        let _ = file.write_all(&[status]);
        drop(file);
        redirect_stdio();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_pidfile_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let lock = LockInner {
            daemon: false,
            wait: false,
            pidfile: Some(dir.path().join("missing/pagers.pid")),
        };
        let cancellation = Cancellation::new();
        cancellation.cancel();

        assert!(hold(&ops::Stats::new(), &lock, &cancellation, None).is_err());
    }
}
