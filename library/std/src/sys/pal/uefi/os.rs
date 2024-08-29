use r_efi::efi::protocols::{device_path, loaded_image_device_path};
use r_efi::efi::Status;

use super::{helpers, unsupported, RawOsError};
use crate::error::Error as StdError;
use crate::ffi::{OsStr, OsString};
use crate::marker::PhantomData;
use crate::os::uefi;
use crate::os::uefi::ffi::OsStringExt;
use crate::path::{self, PathBuf};
use crate::ptr::NonNull;
use crate::{fmt, io};

pub fn errno() -> RawOsError {
    0
}

pub fn error_string(errno: RawOsError) -> String {
    // Keep the List in Alphabetical Order
    // The Messages are taken from UEFI Specification Appendix D - Status Codes
    match r_efi::efi::Status::from_usize(errno) {
        Status::ABORTED => "The operation was aborted.".to_owned(),
        Status::ACCESS_DENIED => "Access was denied.".to_owned(),
        Status::ALREADY_STARTED => "The protocol has already been started.".to_owned(),
        Status::BAD_BUFFER_SIZE => "The buffer was not the proper size for the request.".to_owned(),
        Status::BUFFER_TOO_SMALL => {
                "The buffer is not large enough to hold the requested data. The required buffer size is returned in the appropriate parameter when this error occurs.".to_owned()
        }
        Status::COMPROMISED_DATA => {
                "The security status of the data is unknown or compromised and the data must be updated or replaced to restore a valid security status.".to_owned()
        }
        Status::CONNECTION_FIN => {
                "The receiving operation fails because the communication peer has closed the connection and there is no more data in the receive buffer of the instance.".to_owned()
        }
        Status::CONNECTION_REFUSED => {
                "The receiving or transmission operation fails because this connection is refused.".to_owned()
        }
        Status::CONNECTION_RESET => {
                "The connect fails because the connection is reset either by instance itself or the communication peer.".to_owned()
        }
        Status::CRC_ERROR => "A CRC error was detected.".to_owned(),
        Status::DEVICE_ERROR =>             "The physical device reported an error while attempting the operation.".to_owned()
        ,
        Status::END_OF_FILE => {
            "The end of the file was reached.".to_owned()
        }
        Status::END_OF_MEDIA => {
            "Beginning or end of media was reached".to_owned()
        }
        Status::HOST_UNREACHABLE => {
            "The remote host is not reachable.".to_owned()
        }
        Status::HTTP_ERROR => {
            "A HTTP error occurred during the network operation.".to_owned()
        }
        Status::ICMP_ERROR => {
                "An ICMP error occurred during the network operation.".to_owned()
        }
        Status::INCOMPATIBLE_VERSION => {
                "The function encountered an internal version that was incompatible with a version requested by the caller.".to_owned()
        }
        Status::INVALID_LANGUAGE => {
            "The language specified was invalid.".to_owned()
        }
        Status::INVALID_PARAMETER => {
            "A parameter was incorrect.".to_owned()
        }
        Status::IP_ADDRESS_CONFLICT => {
            "There is an address conflict address allocation".to_owned()
        }
        Status::LOAD_ERROR => {
            "The image failed to load.".to_owned()
        }
        Status::MEDIA_CHANGED => {
                "The medium in the device has changed since the last access.".to_owned()
        }
        Status::NETWORK_UNREACHABLE => {
                "The network containing the remote host is not reachable.".to_owned()
        }
        Status::NO_MAPPING => {
            "A mapping to a device does not exist.".to_owned()
        }
        Status::NO_MEDIA => {
                "The device does not contain any medium to perform the operation.".to_owned()
        }
        Status::NO_RESPONSE => {
                "The server was not found or did not respond to the request.".to_owned()
        }
        Status::NOT_FOUND => "The item was not found.".to_owned(),
        Status::NOT_READY => {
            "There is no data pending upon return.".to_owned()
        }
        Status::NOT_STARTED => {
            "The protocol has not been started.".to_owned()
        }
        Status::OUT_OF_RESOURCES => {
            "A resource has run out.".to_owned()
        }
        Status::PROTOCOL_ERROR => {
                "A protocol error occurred during the network operation.".to_owned()
        }
        Status::PROTOCOL_UNREACHABLE => {
            "An ICMP protocol unreachable error is received.".to_owned()
        }
        Status::SECURITY_VIOLATION => {
                "The function was not performed due to a security violation.".to_owned()
        }
        Status::TFTP_ERROR => {
            "A TFTP error occurred during the network operation.".to_owned()
        }
        Status::TIMEOUT => "The timeout time expired.".to_owned(),
        Status::UNSUPPORTED => {
            "The operation is not supported.".to_owned()
        }
        Status::VOLUME_FULL => {
            "There is no more space on the file system.".to_owned()
        }
        Status::VOLUME_CORRUPTED => {
                "An inconstancy was detected on the file system causing the operating to fail.".to_owned()
        }
        Status::WRITE_PROTECTED => {
            "The device cannot be written to.".to_owned()
        }
        _ => format!("Status: {}", errno),
    }
}

pub fn getcwd() -> io::Result<PathBuf> {
    unsupported()
}

pub fn chdir(_: &path::Path) -> io::Result<()> {
    unsupported()
}

pub struct SplitPaths<'a>(!, PhantomData<&'a ()>);

pub fn split_paths(_unparsed: &OsStr) -> SplitPaths<'_> {
    panic!("unsupported")
}

impl<'a> Iterator for SplitPaths<'a> {
    type Item = PathBuf;
    fn next(&mut self) -> Option<PathBuf> {
        self.0
    }
}

#[derive(Debug)]
pub struct JoinPathsError;

pub fn join_paths<I, T>(_paths: I) -> Result<OsString, JoinPathsError>
where
    I: Iterator<Item = T>,
    T: AsRef<OsStr>,
{
    Err(JoinPathsError)
}

impl fmt::Display for JoinPathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        "not supported on this platform yet".fmt(f)
    }
}

impl StdError for JoinPathsError {}

pub fn current_exe() -> io::Result<PathBuf> {
    let protocol = helpers::image_handle_protocol::<device_path::Protocol>(
        loaded_image_device_path::PROTOCOL_GUID,
    )?;
    helpers::device_path_to_text(protocol).map(PathBuf::from)
}

pub struct Env {
    vars: Vec<(OsString, OsString)>,
    pos: usize,
}

struct EnvIter {
    last_var_name: Vec<u16>,
    last_var_guid: r_efi::efi::Guid,
}

pub struct EnvStrDebug<'a> {
    iter: &'a [(OsString, OsString)],
}

impl fmt::Debug for EnvStrDebug<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        for (a, b) in self.iter {
            list.entry(&(a.to_str().unwrap(), b.to_str().unwrap()));
        }
        list.finish()
    }
}

impl Env {
    // FIXME(https://github.com/rust-lang/rust/issues/114583): Remove this when <OsStr as Debug>::fmt matches <str as Debug>::fmt.
    pub fn str_debug(&self) -> impl fmt::Debug + '_ {
        EnvStrDebug { iter: self.vars.as_slice() }
    }
}

impl Iterator for Env {
    type Item = (OsString, OsString);

    fn next(&mut self) -> Option<(OsString, OsString)> {
        let res = self.vars.get(self.pos)?;
        self.pos += 1;
        Some(res.clone())
    }
}

impl Iterator for EnvIter {
    type Item = (OsString, OsString);

    fn next(&mut self) -> Option<(OsString, OsString)> {
        let (key, guid) =
            uefi_vars::get_next_variable_name(&self.last_var_name, self.last_var_guid).ok()?;

        self.last_var_name = key;
        self.last_var_guid = guid;

        if self.last_var_guid == uefi_vars::SHELL_VARIABLE_GUID {
            let k = OsString::from_wide(&self.last_var_name[..(self.last_var_name.len() - 1)]);
            let v = uefi_vars::get(self.last_var_name.as_mut_slice())?;

            Some((k, v))
        } else {
            self.next()
        }
    }
}

impl fmt::Debug for Env {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.vars).finish()
    }
}

pub fn env() -> Env {
    let iter =
        EnvIter { last_var_name: Vec::from([0]), last_var_guid: uefi_vars::SHELL_VARIABLE_GUID };

    Env { vars: iter.collect(), pos: 0 }
}

pub fn getenv(key: &OsStr) -> Option<OsString> {
    let mut key = uefi_vars::key(key)?;
    uefi_vars::get(key.as_mut_slice())
}

pub unsafe fn setenv(k: &OsStr, v: &OsStr) -> io::Result<()> {
    let mut k =
        uefi_vars::key(k).ok_or(io::const_io_error!(io::ErrorKind::InvalidInput, "Invalid key"))?;
    uefi_vars::set(k.as_mut_slice(), v)
}

pub unsafe fn unsetenv(k: &OsStr) -> io::Result<()> {
    let mut k =
        uefi_vars::key(k).ok_or(io::const_io_error!(io::ErrorKind::InvalidInput, "Invalid key"))?;
    uefi_vars::unset(k.as_mut_slice())
}

pub fn temp_dir() -> PathBuf {
    panic!("no filesystem on this platform")
}

pub fn home_dir() -> Option<PathBuf> {
    None
}

pub fn exit(code: i32) -> ! {
    if let (Some(boot_services), Some(handle)) =
        (uefi::env::boot_services(), uefi::env::try_image_handle())
    {
        let boot_services: NonNull<r_efi::efi::BootServices> = boot_services.cast();
        let _ = unsafe {
            ((*boot_services.as_ptr()).exit)(
                handle.as_ptr(),
                Status::from_usize(code as usize),
                0,
                crate::ptr::null_mut(),
            )
        };
    }
    crate::intrinsics::abort()
}

pub fn getpid() -> u32 {
    panic!("no pids on this platform")
}

mod uefi_vars {
    use super::helpers;
    use crate::ffi::{OsStr, OsString};
    use crate::io;
    use crate::mem::size_of;
    use crate::os::uefi::ffi::{OsStrExt, OsStringExt};
    use crate::ptr::NonNull;

    // Using Shell Variable Guid from edk2/ShellPkg
    // https://github.com/tianocore/edk2/blob/master/ShellPkg/Include/Guid/ShellVariableGuid.h
    pub(crate) const SHELL_VARIABLE_GUID: r_efi::efi::Guid = r_efi::efi::Guid::from_fields(
        0x158def5a,
        0xf656,
        0x419c,
        0xb0,
        0x27,
        &[0x7a, 0x31, 0x92, 0xc0, 0x79, 0xd2],
    );

    pub(crate) fn key(k: &OsStr) -> Option<Vec<u16>> {
        let key = k.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
        if key[..key.len() - 1].contains(&0) {
            return None;
        } else {
            Some(key)
        }
    }

    pub(crate) fn get(key: &mut [u16]) -> Option<OsString> {
        let rt: NonNull<r_efi::efi::RuntimeServices> =
            helpers::runtime_services().expect("UEFI Runtime Services Missing").cast();

        let mut len = 0usize;
        let mut guid = SHELL_VARIABLE_GUID;

        let ret = unsafe {
            ((*rt.as_ptr()).get_variable)(
                key.as_mut_ptr(),
                &mut guid,
                crate::ptr::null_mut(),
                &mut len,
                crate::ptr::null_mut(),
            )
        };

        if ret != r_efi::efi::Status::BUFFER_TOO_SMALL {
            return None;
        }

        let mut val = Vec::<u16>::with_capacity(len / size_of::<u16>());
        let ret = unsafe {
            ((*rt.as_ptr()).get_variable)(
                key.as_mut_ptr(),
                &mut guid,
                crate::ptr::null_mut(),
                &mut len,
                val.as_mut_ptr().cast(),
            )
        };

        if ret.is_error() {
            None
        } else {
            unsafe { val.set_len(len / size_of::<u16>()) };
            Some(OsString::from_wide(&val))
        }
    }

    pub(crate) fn set(key: &mut [u16], val: &OsStr) -> io::Result<()> {
        // UEFI variable value does not need to be NULL terminated.
        let mut val = val.encode_wide().collect::<Vec<u16>>();
        let rt: NonNull<r_efi::efi::RuntimeServices> =
            helpers::runtime_services().expect("UEFI Runtime Services Missing").cast();
        let mut guid = SHELL_VARIABLE_GUID;

        let r = unsafe {
            ((*rt.as_ptr()).set_variable)(
                key.as_mut_ptr(),
                &mut guid,
                r_efi::efi::VARIABLE_BOOTSERVICE_ACCESS,
                val.len() * size_of::<u16>(),
                val.as_mut_ptr().cast(),
            )
        };

        if r.is_error() { Err(io::Error::from_raw_os_error(r.as_usize())) } else { Ok(()) }
    }

    pub(crate) fn unset(key: &mut [u16]) -> io::Result<()> {
        let rt: NonNull<r_efi::efi::RuntimeServices> =
            helpers::runtime_services().expect("UEFI Runtime Services Missing").cast();
        let mut guid = SHELL_VARIABLE_GUID;

        let r = unsafe {
            ((*rt.as_ptr()).set_variable)(
                key.as_mut_ptr(),
                &mut guid,
                r_efi::efi::VARIABLE_BOOTSERVICE_ACCESS,
                0,
                crate::ptr::null_mut(),
            )
        };

        if r.is_error() { Err(io::Error::from_raw_os_error(r.as_usize())) } else { Ok(()) }
    }

    pub(crate) fn get_next_variable_name(
        last_var_name: &[u16],
        last_guid: r_efi::efi::Guid,
    ) -> io::Result<(Vec<u16>, r_efi::efi::Guid)> {
        let mut var_name = Vec::from(last_var_name);
        let mut var_size = var_name.capacity() * size_of::<u16>();
        let mut guid: r_efi::efi::Guid = last_guid;
        let rt: NonNull<r_efi::efi::RuntimeServices> =
            helpers::runtime_services().expect("UEFI Runtime Services Missing").cast();

        let r = unsafe {
            ((*rt.as_ptr()).get_next_variable_name)(&mut var_size, var_name.as_mut_ptr(), &mut guid)
        };

        if !r.is_error() {
            unsafe { var_name.set_len(var_size / size_of::<u16>()) };
            return Ok((var_name, guid));
        }

        if r != r_efi::efi::Status::BUFFER_TOO_SMALL {
            return Err(io::Error::from_raw_os_error(r.as_usize()));
        }

        var_name.reserve((var_size / size_of::<u16>()) - var_name.capacity() + 1);
        var_size = var_name.capacity() * size_of::<u16>();

        let r = unsafe {
            ((*rt.as_ptr()).get_next_variable_name)(&mut var_size, var_name.as_mut_ptr(), &mut guid)
        };

        if r.is_error() {
            Err(io::Error::from_raw_os_error(r.as_usize()))
        } else {
            unsafe { var_name.set_len(var_size / size_of::<u16>()) };
            Ok((var_name, guid))
        }
    }
}
