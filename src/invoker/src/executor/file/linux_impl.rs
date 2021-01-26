use anyhow::Context as _;
use std::{
    ffi::CString,
    io::SeekFrom,
    mem::ManuallyDrop,
    os::unix::io::{FromRawFd, IntoRawFd},
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

pub struct RawFile {
    handle: i32,
}

impl RawFile {
    pub fn from_buffer(buf: &[u8], comment: &str) -> anyhow::Result<RawFile> {
        use nix::{
            fcntl::{self, FcntlArg},
            sys::memfd::{self, MemFdCreateFlag},
        };
        let fd = memfd::memfd_create(
            &CString::new(comment).unwrap(),
            MemFdCreateFlag::MFD_ALLOW_SEALING,
        )
        .context("memfd_create() failed")?;
        let mut buf_rem = buf;
        loop {
            let cnt =
                nix::unistd::write(fd, buf_rem).context("failed to write next chunk of data")?;
            buf_rem = &buf_rem[cnt..];
            if cnt == 0 {
                break;
            }
        }
        // now seal memfd
        // currently this is not important, but when...
        // TODO: cache all this stuff
        // ... it is important that file can't be altered by solution
        let seals =
            libc::F_SEAL_GROW | libc::F_SEAL_SEAL | libc::F_SEAL_WRITE | libc::F_SEAL_SHRINK;
        fcntl::fcntl(
            fd,
            FcntlArg::F_ADD_SEALS(fcntl::SealFlag::from_bits(seals).unwrap()),
        )
        .context("failed to put seals")?;
        // and seek fd to begin
        nix::unistd::lseek64(fd, 0, nix::unistd::Whence::SeekSet)
            .context("failed to seek memfd")?;
        Ok(RawFile { handle: fd })
    }

    pub fn raw(&self) -> u64 {
        self.handle as u64
    }

    pub async fn read_all(&self) -> anyhow::Result<Vec<u8>> {
        let file = unsafe { tokio::fs::File::from_raw_fd(self.handle) };
        // File destructor closes fd
        let mut file = ManuallyDrop::new(file);
        file.seek(SeekFrom::Start(0))
            .await
            .context("failed to seek file to beginning")?;
        let mut out = Vec::new();
        file.read_to_end(&mut out)
            .await
            .context("failed to read file content")?;
        Ok(out)
    }

    pub fn pipe() -> anyhow::Result<(RawFile, RawFile)> {
        let (a, b) = nix::unistd::pipe().context("pipe() failed")?;
        Ok((RawFile { handle: a }, RawFile { handle: b }))
    }

    pub fn open_null() -> anyhow::Result<Self> {
        std::fs::File::open("/dev/null")
            .map_err(Into::into)
            .map(Self::from_std)
    }

    pub fn from_std(f: std::fs::File) -> Self {
        RawFile {
            handle: f.into_raw_fd(),
        }
    }

    pub fn try_clone_inherit(&self) -> anyhow::Result<Self> {
        let out = nix::unistd::dup(self.handle).context("dup(2) failed")?;

        Ok(RawFile { handle: out })
    }
}

impl Drop for RawFile {
    fn drop(&mut self) {
        nix::unistd::close(self.handle).unwrap()
    }
}
