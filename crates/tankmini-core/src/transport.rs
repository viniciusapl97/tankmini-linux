//! ALSA rawmidi transport: a plain character device, no ALSA library needed.

use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::frame::{parse_frames, unwrap_sysex, Frame};

/// An open `/dev/snd/midiC*D*` node.
pub struct Transport {
    file: std::fs::File,
    path: PathBuf,
}

impl Transport {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    fn wait_readable(&self, timeout: Duration) -> io::Result<bool> {
        let mut pfd = libc::pollfd {
            fd: self.fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = timeout.as_millis().min(i32::MAX as u128) as i32;
        let rc = unsafe { libc::poll(&mut pfd, 1, millis) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(rc > 0)
    }

    /// Read whatever is available within `timeout`.
    pub fn read_bytes(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || !self.wait_readable(left)? {
                break;
            }
            let mut chunk = [0u8; 4096];
            match std::io::Read::read(&mut self.file, &mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(buf)
    }

    /// Drop anything already queued (stale replies from a previous request).
    pub fn flush(&mut self) {
        let _ = self.read_bytes(Duration::from_millis(2));
    }

    pub fn write_bytes(&mut self, data: &[u8]) -> io::Result<()> {
        use std::io::Write;
        self.file.write_all(data)?;
        self.file.flush()
    }

    /// Send a frame and collect every frame the device answers within `timeout`.
    pub fn request(&mut self, frame: &Frame, timeout: Duration) -> io::Result<Vec<Frame>> {
        self.flush();
        self.write_bytes(&frame.to_sysex())?;
        let raw = self.read_bytes(timeout)?;
        Ok(parse_frames(&unwrap_sysex(&raw)))
    }

    /// Send a frame and return its first reply, if any.
    pub fn request_one(&mut self, frame: &Frame, timeout: Duration) -> io::Result<Option<Frame>> {
        Ok(self.request(frame, timeout)?.into_iter().next())
    }
}
