use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },

    #[error("{0}")]
    Syscall(#[from] nix::errno::Errno),

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
