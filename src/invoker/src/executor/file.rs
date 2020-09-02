mod linux_impl;

use std::{fs::OpenOptions, mem::ManuallyDrop, path::Path};

use anyhow::Context;
use linux_impl::RawFile;

/// What kind of object `File` instance refers to.
// Currently this is unused, but in future
// it will help for request validation.
#[derive(Clone, Copy)]
enum FileKind {
    /// Anonymous pipe
    Pipe,
    /// Regular file on a file system
    File,
    /// This file is read-only handle to a buffer
    Buf,
}

/// Access mode.
#[derive(Clone, Copy)]
enum Mode {
    /// File is read-only
    Read,
    /// File is write-only
    Write,
    /// File is both readable and writable
    ReadWrite,
}

pub struct File {
    raw: RawFile,
    kind: FileKind,
    mode: Mode,
}

impl File {
    pub fn from_buffer(buf: &[u8], comment: &str) -> anyhow::Result<File> {
        let raw = RawFile::from_buffer(buf, comment)?;

        Ok(File {
            raw,
            kind: FileKind::Buf,
            mode: Mode::Read,
        })
    }

    pub fn into_raw(self) -> u64 {
        let this = ManuallyDrop::new(self);
        this.raw.raw()
    }

    pub fn as_raw(&self) -> u64 {
        self.raw.raw()
    }

    pub fn check_readable(&self) -> anyhow::Result<()> {
        match self.mode {
            Mode::Read | Mode::ReadWrite => Ok(()),
            Mode::Write => anyhow::bail!("File opened in Write mode can't be used for reads"),
        }
    }

    pub fn check_writable(&self) -> anyhow::Result<()> {
        match self.mode {
            Mode::Write | Mode::ReadWrite => Ok(()),
            Mode::Read => anyhow::bail!("File opened in Read mode can't be used for writes"),
        }
    }

    pub async fn read_all(&self) -> anyhow::Result<Vec<u8>> {
        self.check_readable()?;
        self.raw.read_all().await
    }

    pub fn pipe() -> anyhow::Result<(File, File)> {
        let (a, b) = RawFile::pipe()?;
        Ok((
            File {
                raw: a,
                kind: FileKind::Pipe,
                mode: Mode::Read,
            },
            File {
                raw: b,
                kind: FileKind::Pipe,
                mode: Mode::Write,
            },
        ))
    }

    pub fn open_null() -> anyhow::Result<Self> {
        let raw = RawFile::open_null()?;
        Ok(File {
            raw,
            kind: FileKind::File,
            mode: Mode::Read,
        })
    }

    fn open_with(path: &Path, options: &OpenOptions, mode: Mode) -> anyhow::Result<Self> {
        create_parent_dir(path)?;
        let f = options
            .open(path)
            .with_context(|| format!("failed to open {}", path.display()))?;
        Ok(File {
            raw: RawFile::from_std(f),
            kind: FileKind::File,
            mode,
        })
    }

    pub fn open_read_write(path: &Path) -> anyhow::Result<Self> {
        Self::open_with(
            path,
            OpenOptions::new().read(true).write(true).create(true),
            Mode::ReadWrite,
        )
    }

    pub fn open_read(path: &Path) -> anyhow::Result<Self> {
        Self::open_with(path, OpenOptions::new().read(true), Mode::Read)
    }

    pub fn open_write(path: &Path) -> anyhow::Result<Self> {
        Self::open_with(
            path,
            OpenOptions::new().write(true).create(true),
            Mode::Write,
        )
    }

    pub fn try_clone_inherit(&self) -> anyhow::Result<Self> {
        let raw = self.raw.try_clone_inherit()?;

        Ok(File {
            raw,
            kind: self.kind,
            mode: self.mode,
        })
    }
}

fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("file path does not contain parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    Ok(())
}
