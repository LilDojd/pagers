use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pagers_core::{mmap, ops, output};

use crate::RunOp;
use crate::cli::{LockInner, WithCommon};

pub(crate) trait Daemonize: RunOp
where
    Self::Output: 'static,
{
    fn from_args(args: &LockInner) -> Self;

    fn run_daemonized(&self, a: &WithCommon<LockInner>, term: &Arc<AtomicBool>) {
        let notify_fd = go_daemon(a.inner.wait);
        let (stats, _) = self.run(a.common(), false, term);
        hold(&stats, &a.inner, term, notify_fd);
    }
}

impl Daemonize for ops::Lock {
    fn from_args(args: &LockInner) -> Self {
        Self {
            touch: ops::Touch {
                chunk_size: args.load.chunk_size as usize,
                timeout_secs: args.load.timeout,
            },
        }
    }
}

impl Daemonize for ops::Lockall {
    fn from_args(args: &LockInner) -> Self {
        Self {
            lock: ops::Lock::from_args(args),
        }
    }
}

fn go_daemon(wait: bool) -> Option<OwnedFd> {
    let pipe = if wait {
        Some(nix::unistd::pipe().expect("pipe"))
    } else {
        None
    };

    match unsafe { nix::unistd::fork() }.expect("fork") {
        nix::unistd::ForkResult::Parent { child: _ } => {
            if let Some((read_fd, _)) = pipe {
                wait_for_child(read_fd);
            }
            std::process::exit(0);
        }
        nix::unistd::ForkResult::Child => {
            nix::unistd::setsid().expect("setsid");
            if let Some((_, write_fd)) = pipe {
                Some(write_fd)
            } else {
                redirect_stdio();
                None
            }
        }
    }
}

fn wait_for_child(read_fd: OwnedFd) -> ! {
    use std::io::Read;
    let mut file = std::fs::File::from(read_fd);
    let mut buf = [0u8; 1];
    match file.read(&mut buf) {
        Ok(1) => std::process::exit(buf[0] as i32),
        _ => {
            ::tracing::error!("daemon shut down unexpectedly");
            std::process::exit(1);
        }
    }
}

fn redirect_stdio() {
    use std::os::fd::{FromRawFd, OwnedFd};
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

fn hold(
    stats: &ops::Stats,
    inner: &LockInner,
    term: &AtomicBool,
    notify_fd: Option<OwnedFd>,
) {
    if let Some(p) = &inner.pidfile
        && let Err(e) = std::fs::write(p, format!("{}\n", std::process::id()))
    {
        ::tracing::warn!("pidfile: {e}");
    }

    let page_size = mmap::page_size() as i64;
    let total = stats.total_pages.load(std::sync::atomic::Ordering::Relaxed);
    ::tracing::info!(
        "LOCKED {} pages ({})",
        total,
        output::pretty_size(total * page_size)
    );

    if let Some(fd) = notify_fd {
        use std::io::Write;
        let mut file = std::fs::File::from(fd);
        let _ = file.write_all(&[0u8]);
    }

    while !term.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    if let Some(p) = &inner.pidfile {
        let _ = std::fs::remove_file(p);
    }
}
