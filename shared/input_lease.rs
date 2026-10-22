//! Cross-process serialization for native input. No request data is stored.
//! A lease owns a CLOEXEC descriptor; nested helpers receive it only through an
//! explicit Command-scoped delegation. Close-only RAII preserves an inherited
//! flock until the final owner exits; explicit LOCK_UN would break that safety.
use std::cell::RefCell;
use std::ffi::CString;
use std::fmt;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const INHERITED_FD: i32 = 198;
const FD_ENV: &str = "CU_INPUT_LEASE_FD";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Busy,
    Unavailable,
}
impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::Busy => "input_busy",
            Self::Unavailable => "input_lease_unavailable",
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
#[derive(Debug)]
pub struct Lease {
    file: File,
}
thread_local! {static CURRENT:RefCell<Option<File>>=const {RefCell::new(None)};}
fn duplicate(file: &File) -> Result<File, Error> {
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 200) };
    if fd < 0 {
        return Err(Error::Unavailable);
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
impl Lease {
    /// Delegation is scoped to this command. The closure owns a CLOEXEC clone so
    /// dropping the original lease before spawn cannot leave a dangling raw FD.
    pub fn delegate(&self, command: &mut Command) -> Result<(), Error> {
        delegate_file(&self.file, command)
    }
    /// Scope delegation to helpers spawned by the current synchronous CLI
    /// command/batch. This is thread-local state, never process-global env state.
    pub fn with_current<R>(&self, run: impl FnOnce() -> R) -> Result<R, Error> {
        struct Restore(Option<File>);
        impl Drop for Restore {
            fn drop(&mut self) {
                CURRENT.with(|current| {
                    *current.borrow_mut() = self.0.take();
                });
            }
        }
        let replacement = duplicate(&self.file)?;
        let _restore = Restore(CURRENT.with(|current| current.borrow_mut().replace(replacement)));
        Ok(run())
    }
}
/// Called by a central command executor. No active scope means no delegation.
pub fn delegate_current(command: &mut Command) -> Result<(), Error> {
    CURRENT.with(|current| match current.borrow().as_ref() {
        Some(file) => delegate_file(file, command),
        None => Ok(()),
    })
}
fn delegate_file(file: &File, command: &mut Command) -> Result<(), Error> {
    let owned = duplicate(file)?;
    command.env(FD_ENV, INHERITED_FD.to_string());
    // SAFETY: after fork the closure uses only async-signal-safe dup2/fcntl and
    // last_os_error. All allocation, environment setup and FD ownership happen
    // in the parent. The source descriptor is always >=200, distinct from198.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(owned.as_raw_fd(), INHERITED_FD) < 0 {
                return Err(io::Error::last_os_error());
            }
            let flags = libc::fcntl(INHERITED_FD, libc::F_GETFD);
            if flags < 0 || libc::fcntl(INHERITED_FD, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}
/// Acquire before ANY mutable native operation; readonly observation never needs
/// this lease. Caller retains it for the whole workflow/CLI batch, not each key.
pub fn acquire() -> Result<Lease, Error> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(Error::Unavailable)?;
    let delegated = std::env::var_os(FD_ENV)
        .map(|s| s.into_string().map_err(|_| Error::Unavailable))
        .transpose()?;
    acquire_at(&home, delegated.as_deref())
}
pub fn acquire_if_delegated() -> Result<Option<Lease>, Error> {
    if std::env::var_os(FD_ENV).is_some() {
        acquire().map(Some)
    } else {
        Ok(None)
    }
}
pub fn with_current_lease<R>(lease: Lease, run: impl FnOnce() -> R) -> Result<R, Error> {
    lease.with_current(run)
}
fn acquire_at(home: &Path, delegated: Option<&str>) -> Result<Lease, Error> {
    let canonical = open_lock(home)?;
    let file = if let Some(raw) = delegated {
        if raw != INHERITED_FD.to_string() {
            return Err(Error::Unavailable);
        }
        // Do not take ownership until the descriptor has proved that it refers
        // to this exact private canonical lock, not another user's or an alias.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(INHERITED_FD, &mut stat) } != 0 {
            return Err(Error::Unavailable);
        }
        let metadata = canonical.metadata().map_err(|_| Error::Unavailable)?;
        if stat.st_dev as u64 != metadata.dev()
            || stat.st_ino as u64 != metadata.ino()
            || stat.st_uid != unsafe { libc::geteuid() }
            || stat.st_nlink != 1
            || stat.st_mode as u32 & 0o177777 != (libc::S_IFREG as u32 | 0o600)
        {
            return Err(Error::Unavailable);
        }
        // Duplicate before closing the delegated descriptor; thereafter the env
        // marker alone grants nothing and no FD leaks into unrelated children.
        let fd = unsafe { libc::fcntl(INHERITED_FD, libc::F_DUPFD_CLOEXEC, 200) };
        if fd < 0 {
            return Err(Error::Unavailable);
        }
        let file = unsafe { File::from_raw_fd(fd) };
        unsafe {
            libc::close(INHERITED_FD);
        }
        file
    } else {
        canonical
    };
    // CLOEXEC descriptors can still be inherited transiently between another
    // thread's fork and exec. Allow a tiny bounded grace window for that kernel
    // reference to disappear, without ever unlocking or bypassing a real owner.
    for attempt in 0..5 {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Lease { file });
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(Error::Unavailable);
        }
        if attempt == 4 {
            return Err(Error::Busy);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    Err(Error::Busy)
}
fn open_lock(home: &Path) -> Result<File, Error> {
    if !home.is_absolute() {
        return Err(Error::Unavailable);
    }
    let mut dir = File::open("/").map_err(|_| Error::Unavailable)?;
    for component in home.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => {
                let name = CString::new(name.as_encoded_bytes()).map_err(|_| Error::Unavailable)?;
                let fd = unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return Err(Error::Unavailable);
                }
                dir = unsafe { File::from_raw_fd(fd) };
            }
            _ => return Err(Error::Unavailable),
        }
    }
    if dir.metadata().map_err(|_| Error::Unavailable)?.uid() != unsafe { libc::geteuid() } {
        return Err(Error::Unavailable);
    }
    for name in [".cu", "execution"] {
        let name = CString::new(name).unwrap();
        let mut fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 && io::Error::last_os_error().kind() == io::ErrorKind::NotFound {
            if unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) } != 0
                && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists
            {
                return Err(Error::Unavailable);
            }
            fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
        }
        if fd < 0 {
            return Err(Error::Unavailable);
        }
        dir = unsafe { File::from_raw_fd(fd) };
        let metadata = dir.metadata().map_err(|_| Error::Unavailable)?;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o7777 != 0o700 {
            return Err(Error::Unavailable);
        }
    }
    let name = c"input.lock";
    let mut fd = -1;
    for attempt in 0..5 {
        fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd >= 0 {
            break;
        }
        if io::Error::last_os_error().kind() != io::ErrorKind::NotFound || attempt == 4 {
            return Err(Error::Unavailable);
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|_| Error::Unavailable)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(Error::Unavailable);
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().canonicalize().unwrap();
        (dir, home)
    }
    fn acquire_after_release(home: &Path) -> Lease {
        // Other parallel tests may fork while this test owns its CLOEXEC file.
        // Wait only after the intended owner is gone; never use this helper to
        // turn an actual-owner contention assertion into a success.
        let until = Instant::now() + Duration::from_millis(250);
        loop {
            match acquire_at(home, None) {
                Ok(lease) => return lease,
                Err(Error::Busy) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(error) => panic!("lease remained unavailable after owned release: {error}"),
            }
        }
    }
    fn command(home: &Path, mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "input_lease::tests::process_child",
                "--ignored",
                "--nocapture",
            ])
            .env("HOME", home)
            .env("CU_LEASE_TEST_MODE", mode)
            .env_remove(FD_ENV)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    fn succeeds(mut command: Command) {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "child failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    #[ignore = "invoked only by isolated lease subprocess tests"]
    fn process_child() {
        match std::env::var("CU_LEASE_TEST_MODE")
            .expect("owned child test mode")
            .as_str()
        {
            "busy" => assert_eq!(acquire().unwrap_err(), Error::Busy),
            "unavailable" => assert_eq!(acquire().unwrap_err(), Error::Unavailable),
            "independent" => {
                assert!(unsafe { libc::fcntl(INHERITED_FD, libc::F_GETFD) } < 0);
                assert_eq!(acquire().unwrap_err(), Error::Busy);
            }
            "probe" => {
                let _lease = acquire().unwrap();
                assert!(unsafe { libc::fcntl(INHERITED_FD, libc::F_GETFD) } < 0);
            }
            "nested" => {
                let lease = acquire().unwrap();
                assert!(unsafe { libc::fcntl(INHERITED_FD, libc::F_GETFD) } < 0);
                let home = PathBuf::from(std::env::var_os("HOME").unwrap());
                lease
                    .with_current(|| {
                        let mut child = command(&home, "probe");
                        delegate_current(&mut child).unwrap();
                        succeeds(child);
                    })
                    .unwrap();
                let mut child = command(&home, "probe");
                assert!(delegate_current(&mut child).is_ok());
                // Scope restored: delegation alone no longer passes the lease.
                child.env("CU_LEASE_TEST_MODE", "busy");
                succeeds(child);
            }
            "hold" => {
                let _lease = acquire().unwrap();
                let home = PathBuf::from(std::env::var_os("HOME").unwrap());
                fs::write(home.join("child-ready"), b"ready").unwrap();
                std::thread::sleep(Duration::from_secs(5));
            }
            _ => panic!("unknown owned lease test mode"),
        }
    }
    #[test]
    fn independent_process_contends_delegated_and_nested_helpers_share_lease() {
        let (_dir, home) = fixture();
        let lease = acquire_at(&home, None).unwrap();
        succeeds(command(&home, "busy"));
        let mut child = command(&home, "nested");
        lease.delegate(&mut child).unwrap();
        succeeds(child);
        succeeds(command(&home, "independent"));
        assert_eq!(acquire_at(&home, None).unwrap_err(), Error::Busy);
        let inode = fs::metadata(home.join(".cu/execution/input.lock"))
            .unwrap()
            .ino();
        drop(lease);
        drop(acquire_after_release(&home));
        assert_eq!(
            fs::metadata(home.join(".cu/execution/input.lock"))
                .unwrap()
                .ino(),
            inode
        );
    }
    #[test]
    fn cloned_delegation_lives_until_command_drop_and_spawn_failure_releases() {
        let (_dir, home) = fixture();
        let lease = acquire_at(&home, None).unwrap();
        let mut command = Command::new("/cu-nonexistent-lease-test-executable");
        lease.delegate(&mut command).unwrap();
        drop(lease);
        assert_eq!(acquire_at(&home, None).unwrap_err(), Error::Busy);
        assert!(command.spawn().is_err());
        drop(command);
        drop(acquire_after_release(&home));
    }
    #[test]
    fn crashed_process_releases_lease_without_unlinking_canonical_lock() {
        let (_dir, home) = fixture();
        let mut child = command(&home, "hold").spawn().unwrap();
        let started = Instant::now();
        while !home.join("child-ready").is_file() && started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !home.join("child-ready").is_file() {
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!(
                "test child did not acquire lease: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        assert_eq!(acquire_at(&home, None).unwrap_err(), Error::Busy);
        child.kill().unwrap();
        child.wait().unwrap();
        drop(acquire_after_release(&home));
        assert!(home.join(".cu/execution/input.lock").is_file());
    }
    #[test]
    fn delegated_fd_must_match_canonical_private_lock_and_is_never_env_authority() {
        let (_dir, home) = fixture();
        let lease = acquire_at(&home, None).unwrap();
        let mut missing = command(&home, "unavailable");
        missing.env(FD_ENV, "198");
        succeeds(missing);
        let mut malformed = command(&home, "unavailable");
        malformed.env(FD_ENV, "199");
        succeeds(malformed);
        let fake = File::create(home.join("different-lock")).unwrap();
        fake.set_permissions(fs::Permissions::from_mode(0o600))
            .unwrap();
        let mut forged = command(&home, "unavailable");
        delegate_file(&fake, &mut forged).unwrap();
        succeeds(forged);
        drop(lease);
        drop(acquire_after_release(&home));
    }
    #[test]
    fn transient_fork_reference_eventually_releases_without_unlock_or_inode_change() {
        let (_dir, home) = fixture();
        let lease = acquire_at(&home, None).unwrap();
        let inode = fs::metadata(home.join(".cu/execution/input.lock"))
            .unwrap()
            .ino();
        // The child performs no Rust allocation/drop after fork, only the
        // platform sleep syscall and _exit. Its inherited file-table reference
        // reproduces the brief CLOEXEC-before-exec race from parallel spawns.
        let child = unsafe { libc::fork() };
        assert!(child >= 0);
        if child == 0 {
            unsafe {
                let delay = libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 4_000_000,
                };
                libc::nanosleep(&delay, std::ptr::null_mut());
                libc::_exit(0);
            }
        }
        drop(lease);
        let acquired = acquire_after_release(&home);
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(child, &mut status, 0) }, child);
        assert_eq!(status, 0);
        assert_eq!(
            fs::metadata(home.join(".cu/execution/input.lock"))
                .unwrap()
                .ino(),
            inode
        );
        drop(acquired);
    }
    #[test]
    fn private_modes_links_and_hardlinks_rejected_without_chmod() {
        let (_dir, home) = fixture();
        drop(acquire_after_release(&home));
        let execution = home.join(".cu/execution");
        let lock = execution.join("input.lock");
        assert_eq!(fs::metadata(&execution).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&lock).unwrap().mode() & 0o777, 0o600);
        fs::set_permissions(&execution, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(acquire_at(&home, None).unwrap_err(), Error::Unavailable);
        assert_eq!(fs::metadata(&execution).unwrap().mode() & 0o777, 0o755);
        fs::set_permissions(&execution, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(acquire_at(&home, None).unwrap_err(), Error::Unavailable);
        assert_eq!(fs::metadata(&lock).unwrap().mode() & 0o777, 0o644);
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
        let alias = execution.join("alias");
        fs::hard_link(&lock, &alias).unwrap();
        assert_eq!(acquire_at(&home, None).unwrap_err(), Error::Unavailable);
        fs::remove_file(alias).unwrap();
        let target = execution.join("target");
        fs::rename(&lock, &target).unwrap();
        symlink(&target, &lock).unwrap();
        assert_eq!(acquire_at(&home, None).unwrap_err(), Error::Unavailable);
        fs::remove_file(&lock).unwrap();
        fs::rename(&target, &lock).unwrap();
        let old = home.join(".cu-old");
        fs::rename(home.join(".cu"), &old).unwrap();
        symlink(&old, home.join(".cu")).unwrap();
        assert_eq!(acquire_at(&home, None).unwrap_err(), Error::Unavailable);
    }
    #[test]
    fn thread_scopes_restore_nested_lease_without_process_env_mutation() {
        let (_dir, home) = fixture();
        let first = acquire_at(&home, None).unwrap();
        let before = std::env::var_os(FD_ENV);
        first
            .with_current(|| {
                let mut child = command(&home, "probe");
                delegate_current(&mut child).unwrap();
                succeeds(child);
            })
            .unwrap();
        assert_eq!(std::env::var_os(FD_ENV), before);
        let mut independent = command(&home, "busy");
        delegate_current(&mut independent).unwrap();
        succeeds(independent);
    }
}
