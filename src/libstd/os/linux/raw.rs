// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Linux-specific raw type definitions

#![stable(feature = "raw_ext", since = "1.1.0")]

use os::raw;

#[unstable(feature = "raw_linux_arch_dependant_ext",
           reason = "Recently added and incomplete for other types")]
#[cfg(not(any(target_arch = "cris",
              target_arch = "parisc",
              target_arch = "microblaze",
              target_arch = "m68k",
              target_arch = "sh",
              target_arch = "arm",
              target_arch = "avr32",
              target_arch = "sparc",
              target_arch = "frv",
              target_arch = "blackfin",
              target_arch = "mn10300",
              target_arch = "x86",
              target_arch = "x86_64",
              target_arch = "m32r")))]
pub type __kernel_mode_t = raw::c_uint;

#[unstable(feature = "raw_linux_arch_dependant_ext",
           reason = "Recently added and incomplete for other types")]
#[cfg(any(target_arch = "cris",
         target_arch = "parisc",
         target_arch = "microblaze",
         target_arch = "m68k",
         target_arch = "sh",
         target_arch = "arm",
         target_arch = "avr32",
         target_arch = "sparc",
         target_arch = "frv",
         target_arch = "blackfin",
         target_arch = "mn10300",
         target_arch = "x86",
         target_arch = "x86_64",
         target_arch = "m32r"))]
pub type __kernel_mode_t = raw::c_ushort;

#[stable(feature = "raw_ext", since = "1.1.0")] pub type dev_t = u64;
#[stable(feature = "raw_ext", since = "1.1.0")] pub type mode_t = __kernel_mode_t;

#[doc(inline)]
pub use self::arch::{off_t, ino_t, nlink_t, blksize_t, blkcnt_t, stat, time_t};

#[cfg(any(target_arch = "x86",
          target_arch = "le32",
          target_arch = "powerpc",
          target_arch = "arm"))]
mod arch {
    use super::{dev_t, mode_t};
    use os::raw::{c_long, c_short};
    use os::unix::raw::{gid_t, uid_t};

    #[stable(feature = "raw_ext", since = "1.1.0")] pub type blkcnt_t = i32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type blksize_t = i32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type ino_t = u32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type nlink_t = u32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type off_t = i32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type time_t = i32;

    #[repr(C)]
    #[stable(feature = "raw_ext", since = "1.1.0")]
    pub struct stat {
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_dev: dev_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __pad1: c_short,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ino: ino_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mode: mode_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_nlink: nlink_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_uid: uid_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_gid: gid_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_rdev: dev_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __pad2: c_short,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_size: off_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_blksize: blksize_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_blocks: blkcnt_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_atime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_atime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mtime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mtime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ctime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ctime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __unused4: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __unused5: c_long,
    }
}

#[cfg(any(target_arch = "mips",
          target_arch = "mipsel"))]
mod arch {
    use super::mode_t;
    use os::raw::{c_long, c_ulong};
    use os::unix::raw::{gid_t, uid_t};

    #[stable(feature = "raw_ext", since = "1.1.0")] pub type blkcnt_t = i32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type blksize_t = i32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type ino_t = u32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type nlink_t = u32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type off_t = i32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type time_t = i32;

    #[repr(C)]
    #[stable(feature = "raw_ext", since = "1.1.0")]
    pub struct stat {
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_dev: c_ulong,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_pad1: [c_long; 3],
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ino: ino_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mode: mode_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_nlink: nlink_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_uid: uid_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_gid: gid_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_rdev: c_ulong,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_pad2: [c_long; 2],
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_size: off_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_pad3: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_atime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_atime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mtime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mtime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ctime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ctime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_blksize: blksize_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_blocks: blkcnt_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_pad5: [c_long; 14],
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use super::{dev_t, mode_t};
    use os::raw::{c_long, c_int};
    use os::unix::raw::{gid_t, uid_t};

    #[stable(feature = "raw_ext", since = "1.1.0")] pub type blkcnt_t = i64;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type blksize_t = i32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type ino_t = u64;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type nlink_t = u32;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type off_t = i64;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type time_t = i64;

    #[repr(C)]
    #[stable(feature = "raw_ext", since = "1.1.0")]
    pub struct stat {
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_dev: dev_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ino: ino_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mode: mode_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_nlink: nlink_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_uid: uid_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_gid: gid_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_rdev: dev_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __pad1: dev_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_size: off_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_blksize: blksize_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __pad2: c_int,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_blocks: blkcnt_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_atime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_atime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mtime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mtime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ctime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ctime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __unused: [c_int; 2],
    }
}

#[cfg(target_arch = "x86_64")]
mod arch {
    use super::{dev_t, mode_t};
    use os::raw::{c_long, c_int};
    use os::unix::raw::{gid_t, uid_t};

    #[stable(feature = "raw_ext", since = "1.1.0")] pub type blkcnt_t = i64;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type blksize_t = i64;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type ino_t = u64;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type nlink_t = u64;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type off_t = i64;
    #[stable(feature = "raw_ext", since = "1.1.0")] pub type time_t = i64;

    #[repr(C)]
    #[stable(feature = "raw_ext", since = "1.1.0")]
    pub struct stat {
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_dev: dev_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ino: ino_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_nlink: nlink_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mode: mode_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_uid: uid_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_gid: gid_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __pad0: c_int,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_rdev: dev_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_size: off_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_blksize: blksize_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_blocks: blkcnt_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_atime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_atime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mtime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_mtime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ctime: time_t,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub st_ctime_nsec: c_long,
        #[stable(feature = "raw_ext", since = "1.1.0")]
        pub __unused: [c_long; 3],
    }
}
