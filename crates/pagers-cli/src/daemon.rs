use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pagers_core::mincore::{DefaultPageMap, PageMap};
use pagers_core::{ops, output};

use crate::Error;
use crate::cli::{LockInner, WithCommon};
use crate::runop::{Run, run_op};

pub(crate) struct DaemonCmd<'a, O, PM: PageMap = DefaultPageMap> {
    op: O,
    args: &'a WithCommon<LockInner>,
    term: &'a Arc<AtomicBool>,
    _phantom: std::marker::PhantomData<PM>,
}

impl<'a, O: ops::Op + Send + 'static, PM: PageMap + Send + 'static> DaemonCmd<'a, O, PM>
where
    O::Output: 'static,
{
    pub fn new(op: O, args: &'a WithCommon<LockInner>, term: &'a Arc<AtomicBool>) -> Self {
        Self {
            op,
            args,
            term,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<O: ops::Op + Send + 'static, PM: PageMap + Send + 'static> Run for DaemonCmd<'_, O, PM>
where
    O::Output: 'static,
{
    fn run(self) -> Result<(), Error> {
        if self.args.inner.daemon {
            match go_daemon(self.args.inner.wait)? {
                ForkOutcome::Parent => Ok(()),
                ForkOutcome::Child(notify_fd) => {
                    let (stats, _, _) =
                        run_op::<O, PM>(&self.op, self.args.common(), false, self.term)?;
                    hold(&stats, &self.args.inner, self.term, notify_fd);
                    Ok(())
                }
            }
        } else {
            run_op::<O, PM>(&self.op, self.args.common(), false, self.term)?;
            Ok(())
        }
    }
}

enum ForkOutcome {
    Parent,
    Child(Option<OwnedFd>),
}

fn go_daemon(wait: bool) -> Result<ForkOutcome, Error> {
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

fn hold(stats: &ops::Stats, inner: &LockInner, term: &AtomicBool, notify_fd: Option<OwnedFd>) {
    if let Some(p) = &inner.pidfile
        && let Err(e) = std::fs::write(p, format!("{}\n", std::process::id()))
    {
        ::tracing::warn!("pidfile: {e}");
    }

    let page_size = *pagers_core::pagesize::PAGE_SIZE as i64;
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
