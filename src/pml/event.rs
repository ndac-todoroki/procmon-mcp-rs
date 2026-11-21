//! A single captured event and the names of its class and operation.

/// Event classes as stored in the event header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventClass {
    Unknown,
    Process,
    Registry,
    FileSystem,
    Profiling,
    Network,
    /// Named pipes and mailslots. Uses the file system operation codes and layout.
    Ipc,
}

impl EventClass {
    pub(crate) fn from_u32(v: u32) -> EventClass {
        match v {
            1 => EventClass::Process,
            2 => EventClass::Registry,
            3 => EventClass::FileSystem,
            4 => EventClass::Profiling,
            5 => EventClass::Network,
            6 => EventClass::Ipc,
            _ => EventClass::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    /// Key into the process table.
    pub process_index: u32,
    pub tid: u32,
    pub class: EventClass,
    pub operation: u16,
    /// 100 ns ticks.
    pub duration: u64,
    /// FILETIME (100 ns ticks since 1601-01-01 UTC).
    pub timestamp: u64,
    /// NTSTATUS.
    pub result: u32,
    /// The Path column. `None` for operations without a path.
    pub path: Option<String>,
    pub detail: Detail,
}

/// The parts of the Detail column this crate understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    None,
    /// ReadFile / WriteFile.
    ReadWrite {
        offset: i64,
        length: u32,
    },
}

impl Event {
    /// The operation name as Process Monitor displays it.
    ///
    /// File system sub-operations (e.g. `QueryBasicInformationFile`) are not resolved;
    /// the base operation name is returned for them.
    pub fn operation_name(&self) -> &'static str {
        let table: &[&str] = match self.class {
            EventClass::Process => PROCESS_OPERATIONS,
            EventClass::Registry => REGISTRY_OPERATIONS,
            EventClass::FileSystem | EventClass::Ipc => FILESYSTEM_OPERATIONS,
            EventClass::Profiling => PROFILING_OPERATIONS,
            EventClass::Network => NETWORK_OPERATIONS,
            EventClass::Unknown => &[],
        };
        table
            .get(usize::from(self.operation))
            .copied()
            .unwrap_or("<Unknown>")
    }
}

// Operation tables indexed by operation code, from procmon-parser's consts.py.

const PROCESS_OPERATIONS: &[&str] = &[
    "Process Defined",
    "Process Create",
    "Process Exit",
    "Thread Create",
    "Thread Exit",
    "Load Image",
    "Thread Profile",
    "Process Start",
    "Process Statistics",
    "System Statistics",
];

const REGISTRY_OPERATIONS: &[&str] = &[
    "RegOpenKey",
    "RegCreateKey",
    "RegCloseKey",
    "RegQueryKey",
    "RegSetValue",
    "RegQueryValue",
    "RegEnumValue",
    "RegEnumKey",
    "RegSetInfoKey",
    "RegDeleteKey",
    "RegDeleteValue",
    "RegFlushKey",
    "RegLoadKey",
    "RegUnloadKey",
    "RegRenameKey",
    "RegQueryMultipleValueKey",
    "RegSetKeySecurity",
    "RegQueryKeySecurity",
];

const PROFILING_OPERATIONS: &[&str] = &[
    "Thread Profiling",
    "Process Profiling",
    "Debug Output Profiling",
];

// Procmon prefixes these with "TCP " or "UDP " based on the event details.
const NETWORK_OPERATIONS: &[&str] = &[
    "Unknown",
    "Other",
    "Send",
    "Receive",
    "Accept",
    "Connect",
    "Disconnect",
    "Reconnect",
    "Retransmit",
    "TCPCopy",
];

const FILESYSTEM_OPERATIONS: &[&str] = &[
    "VolumeDismount",
    "VolumeMount",
    "FASTIO_MDL_WRITE_COMPLETE",
    "WriteFile2",
    "FASTIO_MDL_READ_COMPLETE",
    "ReadFile2",
    "QueryOpen",
    "FASTIO_CHECK_IF_POSSIBLE",
    "IRP_MJ_12",
    "IRP_MJ_11",
    "IRP_MJ_10",
    "IRP_MJ_9",
    "IRP_MJ_8",
    "FASTIO_NOTIFY_STREAM_FO_CREATION",
    "FASTIO_RELEASE_FOR_CC_FLUSH",
    "FASTIO_ACQUIRE_FOR_CC_FLUSH",
    "FASTIO_RELEASE_FOR_MOD_WRITE",
    "FASTIO_ACQUIRE_FOR_MOD_WRITE",
    "FASTIO_RELEASE_FOR_SECTION_SYNCHRONIZATION",
    "CreateFileMapping",
    "CreateFile",
    "CreatePipe",
    "IRP_MJ_CLOSE",
    "ReadFile",
    "WriteFile",
    "QueryInformationFile",
    "SetInformationFile",
    "QueryEAFile",
    "SetEAFile",
    "FlushBuffersFile",
    "QueryVolumeInformation",
    "SetVolumeInformation",
    "DirectoryControl",
    "FileSystemControl",
    "DeviceIoControl",
    "InternalDeviceIoControl",
    "Shutdown",
    "LockUnlockFile",
    "CloseFile",
    "CreateMailSlot",
    "QuerySecurityFile",
    "SetSecurityFile",
    "Power",
    "SystemControl",
    "DeviceChange",
    "QueryFileQuota",
    "SetFileQuota",
    "PlugAndPlay",
];
