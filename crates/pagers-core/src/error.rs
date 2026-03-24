use std::{num::TryFromIntError, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },

    #[error("{0}")]
    Syscall(#[from] nix::errno::Errno),

    #[error("{0}")]
    TryFromInt(#[from] TryFromIntError),

    #[error("{path}: offset {offset} beyond file size {file_len}")]
    OffsetBeyondFile {
        path: PathBuf,
        offset: u64,
        file_len: u64,
    },
}

impl Error {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Self::Io {
            context: String::new(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_display_with_context() {
        let err = Error::io(
            "/tmp/test.dat",
            std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        );
        let msg = err.to_string();
        assert!(msg.contains("/tmp/test.dat"), "msg: {msg}");
        assert!(msg.contains("file not found"), "msg: {msg}");
    }

    #[test]
    fn test_io_error_from_std() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io { .. }));
    }

    #[test]
    fn test_syscall_error_from_errno() {
        let err: Error = nix::errno::Errno::EBADF.into();
        assert!(matches!(err, Error::Syscall(_)));
        assert!(err.to_string().contains("EBADF"));
    }

    #[test]
    fn test_offset_beyond_file_display() {
        let err = Error::OffsetBeyondFile {
            path: PathBuf::from("/data/big.bin"),
            offset: 1000,
            file_len: 500,
        };
        let msg = err.to_string();
        assert!(msg.contains("/data/big.bin"), "msg: {msg}");
        assert!(msg.contains("1000"), "msg: {msg}");
        assert!(msg.contains("500"), "msg: {msg}");
    }
}
