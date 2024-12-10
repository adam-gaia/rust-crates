#[derive(Debug)]
#[repr(i32)]
pub enum Error {
    /// Operation not permitted.
    OperationNotPermitted = libc::EPERM,
    /// No such file or directory.
    NoSuchFileOrDirectory = libc::ENOENT,
    /// No such process.
    NoSuchProcess = libc::ESRCH,
    /// Interrupted system call.
    InterruptedSystemCall = libc::EINTR,
    /// Input/output error.
    IOError = libc::EIO,
    /// No such device or address.
    NoSuchDeviceOrAddress = libc::ENXIO,
    /// Argument list too long.
    ArgumentListTooLong = libc::E2BIG,
    /// Exec format error.
    ExecFormatError = libc::ENOEXEC,
    /// Bad file descriptor.
    BadFileDescriptor = libc::EBADF,
    /// No child processes.
    NoChildProcesses = libc::ECHILD,
    /// Resource temporarily unavailable.
    ResourceUnavailable = libc::EAGAIN,
    /// Out of memory.
    OutOfMemory = libc::ENOMEM,
    /// Permission denied.
    PermissionDenied = libc::EACCES,
    /// Bad address.
    BadAddress = libc::EFAULT,
    /// Block device required.
    BlockDeviceRequired = libc::ENOTBLK,
    /// Device or resource busy.
    DeviceOrResourceBusy = libc::EBUSY,
    /// File exists.
    FileExists = libc::EEXIST,
    /// Invalid cross-device link.
    InvalidCrossDeviceLink = libc::EXDEV,
    /// No such device.
    NoSuchDevice = libc::ENODEV,
    /// Not a directory.
    NotADirectory = libc::ENOTDIR,
    /// Is a directory.
    IsADirectory = libc::EISDIR,
    /// Invalid argument.
    InvalidArgument = libc::EINVAL,
    /// Too many open files in system.
    TooManyOpenFilesSystem = libc::ENFILE,
    /// Too many open files.
    TooManyOpenFiles = libc::EMFILE,
    /// Inappropriate ioctl for device.
    InappropriateIoctl = libc::ENOTTY,
    /// Text file busy.
    TextFileBusy = libc::ETXTBSY,
    /// File too large.
    FileTooLarge = libc::EFBIG,
    /// No space left on device.
    NoSpaceLeftOnDevice = libc::ENOSPC,
    /// Illegal seek.
    IllegalSeek = libc::ESPIPE,
    /// Read-only file system.
    ReadOnlyFileSystem = libc::EROFS,
    /// Too many links.
    TooManyLinks = libc::EMLINK,
    /// Broken pipe.
    BrokenPipe = libc::EPIPE,
    /// Numerical argument out of domain.
    NumericalArgumentOutOfDomain = libc::EDOM,
    /// Result too large.
    ResultTooLarge = libc::ERANGE,
    /// Function not implemented.
    FunctionNotImplemented = libc::ENOSYS,
}

impl From<Error> for i32 {
    fn from(err: Error) -> i32 {
        err as i32
    }
}
