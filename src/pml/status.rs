//! NTSTATUS names as Process Monitor displays them (subset of procmon-parser's table).

/// The Result column text for an NTSTATUS, if it is one we know.
pub fn status_name(code: u32) -> Option<&'static str> {
    Some(match code {
        0x0000_0000 => "SUCCESS",
        0x0000_0104 => "REPARSE",
        0x8000_0005 => "BUFFER OVERFLOW",
        0x8000_0006 => "NO MORE FILES",
        0xC000_0001 => "UNSUCCESSFUL",
        0xC000_0008 => "INVALID HANDLE",
        0xC000_000F => "NO SUCH FILE",
        0xC000_0011 => "END OF FILE",
        0xC000_0022 => "ACCESS DENIED",
        0xC000_0034 => "NAME NOT FOUND",
        0xC000_0035 => "NAME COLLISION",
        0xC000_003A => "PATH NOT FOUND",
        0xC000_0043 => "SHARING VIOLATION",
        0xC000_0056 => "DELETE PENDING",
        0xC000_00BA => "IS DIRECTORY",
        0xC000_0120 => "CANCELLED",
        0xC000_0225 => "NOT FOUND",
        0xC000_0275 => "NOT REPARSE POINT",
        0xC01C_0004 => "FAST IO DISALLOWED",
        _ => return None,
    })
}
