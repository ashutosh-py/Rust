// Copyright 2014-2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use error::prelude::*;
use inner::prelude::*;
use os_str::prelude::*;
use env::prelude::*;
use c_str::*;
use collections::borrow::ToOwned;
use collections::btree_map::BTreeMap;
use collections::Vec;
use core::fmt;
use libc::{self, c_int};
use core::mem;
use core::ptr;
use unix::fd::FileDesc;
use unix::pipe::{self, AnonPipe};
use unix::env::environ;
use unix::cvt_r;
//use sys::unix::fs::{File, OpenOptions};
use unix::c;
use process::{self as sys, Stdio};
use io;
//use sys::fs::{OpenOptions as sys_OpenOptions, File as sys_File};

////////////////////////////////////////////////////////////////////////////////
// Command
////////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub struct Command {
    pub program: CString,
    pub args: Vec<CString>,
    pub env: Option<BTreeMap<OsString, OsString>>,
    pub cwd: Option<CString>,
    pub uid: Option<libc::uid_t>,
    pub gid: Option<libc::gid_t>,
    pub session_leader: bool,
}

impl sys::Command for Command {
    fn new(program: &OsStr) -> Result<Command> {
        Ok(Command {
            program: try!(CString::new(program.as_bytes())),
            args: Vec::new(),
            env: None,
            cwd: None,
            uid: None,
            gid: None,
            session_leader: false,
        })
    }

    fn arg(&mut self, arg: &OsStr) {
        self.args.push(CString::new(arg.as_bytes()).unwrap());
    }
    fn args<'a, I: Iterator<Item = &'a OsStr>>(&mut self, args: I) {
        self.args.extend(args.map(|s| CString::new(s.as_bytes()).unwrap()))
    }
    fn env(&mut self, key: &OsStr, val: &OsStr) {
        self.init_env_map();
        self.env.as_mut().unwrap().insert(key.to_owned(), val.to_owned());
    }
    fn env_remove(&mut self, key: &OsStr) {
        self.init_env_map();
        self.env.as_mut().unwrap().remove(key);
    }
    fn env_clear(&mut self) {
        self.env = Some(BTreeMap::new())
    }
    fn cwd(&mut self, dir: &OsStr) {
        self.cwd = Some(CString::new(dir.as_bytes()).unwrap())
    }
}

impl Command {
    fn init_env_map(&mut self) {
        if self.env.is_none() {
            self.env = Some(Env::env().unwrap().collect());
        }
    }
}

impl fmt::Debug for Command {
    /// Format the program and arguments of a Command for display. Any
    /// non-utf8 data is lossily converted using the utf8 replacement
    /// character.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        /*try!(write!(f, "{:?}", self.program));
        for arg in &self.args {
            try!(write!(f, " {:?}", arg));
        }*/
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Processes
////////////////////////////////////////////////////////////////////////////////

/// Unix exit statuses
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum ExitStatus {
    /// Normal termination with an exit code.
    Code(i32),

    /// Termination by signal, with the signal number.
    ///
    /// Never generated on Windows.
    Signal(i32),
}

impl sys::ExitStatus for ExitStatus {
    fn success(&self) -> bool {
        *self == ExitStatus::Code(0)
    }

    fn code(&self) -> Option<i32> {
        match *self {
            ExitStatus::Code(c) => Some(c),
            _ => None
        }
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ExitStatus::Code(code) =>  write!(f, "exit code: {}", code),
            ExitStatus::Signal(code) =>  write!(f, "signal: {}", code),
        }
    }
}

pub enum StdioImp {
    Fd(FileDesc),
    Raw(c_int),
    Inherit,
    None,
}

impl From<Stdio<Process>> for StdioImp {
    fn from(stdio: Stdio<Process>) -> Self {
        match stdio {
            Stdio::MakePipe | Stdio::None => StdioImp::None,
            Stdio::Inherit => StdioImp::Inherit,
            Stdio::Raw(fd) => StdioImp::Raw(fd),
        }
    }
}

/// The unique id of the process (this should never be negative).
pub struct Process {
    pid: libc::pid_t,
    stdin_pipe: Option<AnonPipe>,
    stdout_pipe: Option<AnonPipe>,
    stderr_pipe: Option<AnonPipe>,
}

const CLOEXEC_MSG_FOOTER: &'static [u8] = b"NOEX";

impl sys::Process for Process {
    type RawFd = c_int;
    type Command = Command;
    type ExitStatus = ExitStatus;
    type PipeRead = AnonPipe;
    type PipeWrite = AnonPipe;

    fn exit(code: i32) -> ! {
        unsafe { libc::_exit(code as c_int) }
    }

    unsafe fn kill(&self) -> Result<()> {
        if libc::funcs::posix88::signal::kill(self.pid, libc::SIGKILL) < 0 {
            Error::expect_last_result()
        } else {
            Ok(())
        }
    }

    fn stdin(&mut self) -> &mut Option<Self::PipeWrite> { &mut self.stdin_pipe }
    fn stdout(&mut self) -> &mut Option<Self::PipeRead> { &mut self.stdout_pipe }
    fn stderr(&mut self) -> &mut Option<Self::PipeRead> { &mut self.stderr_pipe }

    fn spawn(cfg: &Self::Command, stdin: Stdio<Self>, stdout: Stdio<Self>, stderr: Stdio<Self>) -> Result<Self> {
        let dirp = cfg.cwd.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());

        let (envp, _a, _b) = make_envp(cfg.env.as_ref());
        let (argv, _a) = make_argv(&cfg.program, &cfg.args);
        let (input, output) = try!(pipe::anon_pipe());

        let (stdin, stdin_pipe) = match stdin {
            Stdio::MakePipe => { let (r, w) = try!(pipe::anon_pipe()); (StdioImp::Fd(r.into_inner()), Some(w)) },
            stdio => (stdio.into(), None),
        };

        let (stdout, stdout_pipe) = match stdout {
            Stdio::MakePipe => { let (r, w) = try!(pipe::anon_pipe()); (StdioImp::Fd(w.into_inner()), Some(r)) },
            stdio => (stdio.into(), None),
        };

        let (stderr, stderr_pipe) = match stderr {
            Stdio::MakePipe => { let (r, w) = try!(pipe::anon_pipe()); (StdioImp::Fd(w.into_inner()), Some(r)) },
            stdio => (stdio.into(), None),
        };

        let pid = unsafe {
            match libc::fork() {
                0 => {
                    drop(input);
                    Process::child_after_fork(cfg, output, argv, envp, dirp,
                                              stdin, stdout, stderr)
                }
                n if n < 0 => return Error::expect_last_result(),
                n => n,
            }
        };

        let p = Process { pid: pid, stdin_pipe: stdin_pipe, stdout_pipe: stdout_pipe, stderr_pipe: stderr_pipe };
        drop(output);
        let mut bytes = [0; 8];

        // loop to handle EINTR
        loop {
            match io::Read::read(&input, &mut bytes) {
                Ok(0) => return Ok(p),
                Ok(8) => {
                    assert!(combine(CLOEXEC_MSG_FOOTER) == combine(&bytes[4.. 8]),
                            "Validation on the CLOEXEC pipe failed: {:?}", bytes);
                    let errno = combine(&bytes[0.. 4]);
                    assert!(p.wait().is_ok(),
                            "wait() should either return Ok or panic");
                    return Err(Error::from_code(errno))
                }
                Err(ref e) if e.code() == libc::EINTR => {}
                Err(e) => {
                    assert!(p.wait().is_ok(),
                            "wait() should either return Ok or panic");
                    panic!("the CLOEXEC pipe failed: {:?}", e)
                },
                Ok(..) => { // pipe I/O up to PIPE_BUF bytes should be atomic
                    assert!(p.wait().is_ok(),
                            "wait() should either return Ok or panic");
                    panic!("short read on the CLOEXEC pipe")
                }
            }
        }

        fn combine(arr: &[u8]) -> i32 {
            let a = arr[0] as u32;
            let b = arr[1] as u32;
            let c = arr[2] as u32;
            let d = arr[3] as u32;

            ((a << 24) | (b << 16) | (c << 8) | (d << 0)) as i32
        }
    }

    fn id(&self) -> Result<u32> {
        Ok(self.pid as u32)
    }

    fn wait(&self) -> Result<ExitStatus> {
        let mut status = 0 as c_int;
        try!(cvt_r(|| unsafe { c::waitpid(self.pid, &mut status, 0) }));
        Ok(translate_status(status))
    }

    fn try_wait(&self) -> Option<ExitStatus> {
        let mut status = 0 as c_int;
        match cvt_r(|| unsafe {
            c::waitpid(self.pid, &mut status, c::WNOHANG)
        }) {
            Ok(0) => None,
            Ok(n) if n == self.pid => Some(translate_status(status)),
            Ok(n) => panic!("unknown pid: {}", n),
            Err(e) => panic!("unknown waitpid error: {}", e),
        }
    }
}

impl Process {
    // And at this point we've reached a special time in the life of the
    // child. The child must now be considered hamstrung and unable to
    // do anything other than syscalls really. Consider the following
    // scenario:
    //
    //      1. Thread A of process 1 grabs the malloc() mutex
    //      2. Thread B of process 1 forks(), creating thread C
    //      3. Thread C of process 2 then attempts to malloc()
    //      4. The memory of process 2 is the same as the memory of
    //         process 1, so the mutex is locked.
    //
    // This situation looks a lot like deadlock, right? It turns out
    // that this is what pthread_atfork() takes care of, which is
    // presumably implemented across platforms. The first thing that
    // threads to *before* forking is to do things like grab the malloc
    // mutex, and then after the fork they unlock it.
    //
    // Despite this information, libnative's spawn has been witnessed to
    // deadlock on both OSX and FreeBSD. I'm not entirely sure why, but
    // all collected backtraces point at malloc/free traffic in the
    // child spawned process.
    //
    // For this reason, the block of code below should contain 0
    // invocations of either malloc of free (or their related friends).
    //
    // As an example of not having malloc/free traffic, we don't close
    // this file descriptor by dropping the FileDesc (which contains an
    // allocation). Instead we just close it manually. This will never
    // have the drop glue anyway because this code never returns (the
    // child will either exec() or invoke libc::exit)
    unsafe fn child_after_fork(cfg: &Command,
                               mut output: AnonPipe,
                               argv: *const *const libc::c_char,
                               envp: *const libc::c_void,
                               dirp: *const libc::c_char,
                               stdin: StdioImp,
                               stdout: StdioImp,
                               stderr: StdioImp) -> ! {
        fn fail(output: &mut AnonPipe) -> ! {
            let errno = Error::last_error().map(|e| e.code()).unwrap_or(0) as u32;
            let bytes = [
                (errno >> 24) as u8,
                (errno >> 16) as u8,
                (errno >>  8) as u8,
                (errno >>  0) as u8,
                CLOEXEC_MSG_FOOTER[0], CLOEXEC_MSG_FOOTER[1],
                CLOEXEC_MSG_FOOTER[2], CLOEXEC_MSG_FOOTER[3]
            ];
            // pipe I/O up to PIPE_BUF bytes should be atomic, and then we want
            // to be sure we *don't* run at_exit destructors as we're being torn
            // down regardless
            assert!(io::Write::write(output, &bytes).is_ok());
            unsafe { libc::_exit(1) }
        }

        let setup = |src: StdioImp, dst: c_int| {
            match src {
                StdioImp::Inherit => true,
                StdioImp::Raw(fd) => cvt_r(|| libc::dup2(fd, dst)).is_ok(),
                StdioImp::Fd(fd) => cvt_r(|| libc::dup2(*fd.as_inner(), dst)).is_ok(),

                // If a stdio file descriptor is set to be ignored, we open up
                // /dev/null into that file descriptor. Otherwise, the first
                // file descriptor opened up in the child would be numbered as
                // one of the stdio file descriptors, which is likely to wreak
                // havoc.
                StdioImp::None => {
                    false
                    /*let mut opts = OpenOptions::new();
                    opts.read(dst == libc::STDIN_FILENO);
                    opts.write(dst != libc::STDIN_FILENO);
                    let devnull = CStr::from_ptr(b"/dev/null\0".as_ptr()
                                                    as *const _);
                    if let Ok(f) = File::open_c(devnull, &opts) {
                        cvt_r(|| libc::dup2(f.fd().raw(), dst)).is_ok()
                    } else {
                        false
                    }*/
                }
            }
        };

        if !setup(stdin, libc::STDIN_FILENO) { fail(&mut output) }
        if !setup(stdout, libc::STDOUT_FILENO) { fail(&mut output) }
        if !setup(stderr, libc::STDERR_FILENO) { fail(&mut output) }

        if let Some(u) = cfg.gid {
            if libc::setgid(u as libc::gid_t) != 0 {
                fail(&mut output);
            }
        }
        if let Some(u) = cfg.uid {
            // When dropping privileges from root, the `setgroups` call
            // will remove any extraneous groups. If we don't call this,
            // then even though our uid has dropped, we may still have
            // groups that enable us to do super-user things. This will
            // fail if we aren't root, so don't bother checking the
            // return value, this is just done as an optimistic
            // privilege dropping function.
            let _ = c::setgroups(0, ptr::null());

            if libc::setuid(u as libc::uid_t) != 0 {
                fail(&mut output);
            }
        }
        if cfg.session_leader {
            // Don't check the error of setsid because it fails if we're the
            // process leader already. We just forked so it shouldn't return
            // error, but ignore it anyway.
            let _ = libc::setsid();
        }
        if !dirp.is_null() && libc::chdir(dirp) == -1 {
            fail(&mut output);
        }
        if !envp.is_null() {
            *environ() = envp as *const _;
        }

        // Reset signal handling so the child process starts in a
        // standardized state. libstd ignores SIGPIPE, and signal-handling
        // libraries often set a mask. Child processes inherit ignored
        // signals and the signal mask from their parent, but most
        // UNIX programs do not reset these things on their own, so we
        // need to clean things up now to avoid confusing the program
        // we're about to run.
        let mut set: c::sigset_t = mem::uninitialized();
        if c::sigemptyset(&mut set) != 0 ||
           c::pthread_sigmask(c::SIG_SETMASK, &set, ptr::null_mut()) != 0 ||
           libc::funcs::posix01::signal::signal(
               libc::SIGPIPE, mem::transmute(c::SIG_DFL)
           ) == mem::transmute(c::SIG_ERR) {
            fail(&mut output);
        }

        let _ = libc::execvp(*argv, argv);
        fail(&mut output)
    }

}

fn make_argv(prog: &CString, args: &[CString])
             -> (*const *const libc::c_char, Vec<*const libc::c_char>)
{
    let mut ptrs: Vec<*const libc::c_char> = Vec::with_capacity(args.len()+1);

    // Convert the CStrings into an array of pointers. Note: the
    // lifetime of the various CStrings involved is guaranteed to be
    // larger than the lifetime of our invocation of cb, but this is
    // technically unsafe as the callback could leak these pointers
    // out of our scope.
    ptrs.push(prog.as_ptr());
    ptrs.extend(args.iter().map(|tmp| tmp.as_ptr()));

    // Add a terminating null pointer (required by libc).
    ptrs.push(ptr::null());

    (ptrs.as_ptr(), ptrs)
}

fn make_envp(env: Option<&BTreeMap<OsString, OsString>>)
             -> (*const libc::c_void, Vec<Vec<u8>>, Vec<*const libc::c_char>)
{
    // On posixy systems we can pass a char** for envp, which is a
    // null-terminated array of "k=v\0" strings. Since we must create
    // these strings locally, yet expose a raw pointer to them, we
    // create a temporary vector to own the CStrings that outlives the
    // call to cb.
    if let Some(env) = env {
        let mut tmps = Vec::with_capacity(env.len());

        for pair in env {
            let mut kv = Vec::new();
            kv.push_all(pair.0.as_bytes());
            kv.push('=' as u8);
            kv.push_all(pair.1.as_bytes());
            kv.push(0); // terminating null
            tmps.push(kv);
        }

        let mut ptrs: Vec<*const libc::c_char> =
            tmps.iter()
                .map(|tmp| tmp.as_ptr() as *const libc::c_char)
                .collect();
        ptrs.push(ptr::null());

        (ptrs.as_ptr() as *const _, tmps, ptrs)
    } else {
        (ptr::null(), Vec::new(), Vec::new())
    }
}

fn translate_status(status: c_int) -> ExitStatus {
    #![allow(non_snake_case)]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    mod imp {
        pub fn WIFEXITED(status: i32) -> bool { (status & 0xff) == 0 }
        pub fn WEXITSTATUS(status: i32) -> i32 { (status >> 8) & 0xff }
        pub fn WTERMSIG(status: i32) -> i32 { status & 0x7f }
    }

    #[cfg(any(target_os = "macos",
              target_os = "ios",
              target_os = "freebsd",
              target_os = "dragonfly",
              target_os = "bitrig",
              target_os = "netbsd",
              target_os = "openbsd"))]
    mod imp {
        pub fn WIFEXITED(status: i32) -> bool { (status & 0x7f) == 0 }
        pub fn WEXITSTATUS(status: i32) -> i32 { status >> 8 }
        pub fn WTERMSIG(status: i32) -> i32 { status & 0o177 }
    }

    if imp::WIFEXITED(status) {
        ExitStatus::Code(imp::WEXITSTATUS(status))
    } else {
        ExitStatus::Signal(imp::WTERMSIG(status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prelude::v1::*;

    use ffi::OsStr;
    use mem;
    use ptr;
    use libc;
    use slice;
    use sys::{self, c, cvt, pipe};

    macro_rules! t {
        ($e:expr) => {
            match $e {
                Ok(t) => t,
                Err(e) => panic!("received error for `{}`: {}", stringify!($e), e),
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    extern {
        #[cfg_attr(target_os = "netbsd", link_name = "__sigaddset14")]
        fn sigaddset(set: *mut c::sigset_t, signum: libc::c_int) -> libc::c_int;
    }

    #[cfg(target_os = "android")]
    unsafe fn sigaddset(set: *mut c::sigset_t, signum: libc::c_int) -> libc::c_int {
        let raw = slice::from_raw_parts_mut(set as *mut u8, mem::size_of::<c::sigset_t>());
        let bit = (signum - 1) as usize;
        raw[bit / 8] |= 1 << (bit % 8);
        return 0;
    }

    // See #14232 for more information, but it appears that signal delivery to a
    // newly spawned process may just be raced in the OSX, so to prevent this
    // test from being flaky we ignore it on OSX.
    #[test]
    #[cfg_attr(target_os = "macos", ignore)]
    fn test_process_mask() {
        unsafe {
            // Test to make sure that a signal mask does not get inherited.
            let cmd = Command::new(OsStr::new("cat"));
            let (stdin_read, stdin_write) = t!(pipe::anon_pipe());
            let (stdout_read, stdout_write) = t!(pipe::anon_pipe());

            let mut set: c::sigset_t = mem::uninitialized();
            let mut old_set: c::sigset_t = mem::uninitialized();
            t!(cvt(c::sigemptyset(&mut set)));
            t!(cvt(sigaddset(&mut set, libc::SIGINT)));
            t!(cvt(c::pthread_sigmask(c::SIG_SETMASK, &set, &mut old_set)));

            let cat = t!(Process::spawn(&cmd, Stdio::Raw(stdin_read.raw()),
                                              Stdio::Raw(stdout_write.raw()),
                                              Stdio::None));
            drop(stdin_read);
            drop(stdout_write);

            t!(cvt(c::pthread_sigmask(c::SIG_SETMASK, &old_set,
                                      ptr::null_mut())));

            t!(cvt(libc::funcs::posix88::signal::kill(cat.id() as libc::pid_t,
                                                      libc::SIGINT)));
            // We need to wait until SIGINT is definitely delivered. The
            // easiest way is to write something to cat, and try to read it
            // back: if SIGINT is unmasked, it'll get delivered when cat is
            // next scheduled.
            let _ = io::Write::write(&stdin_write, b"Hello");
            drop(stdin_write);

            // Either EOF or failure (EPIPE) is okay.
            let mut buf = [0; 5];
            if let Ok(ret) = io::Read::read(&stdout_read, &mut buf) {
                assert!(ret == 0);
            }

            t!(cat.wait());
        }
    }
}
