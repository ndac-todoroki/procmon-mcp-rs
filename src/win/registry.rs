//! Saving and restoring the values of a registry key.
//!
//! Process Monitor writes the configuration it was started with back to
//! `HKCU\Software\Sysinternals\Process Monitor` when it exits, which would replace the
//! user's own GUI settings with the capture's filter. The runner saves the key first and
//! puts it back afterwards. Install writes its "Installed apps" entry under HKLM the same way.

use std::io;

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_SZ, REG_VALUE_TYPE, RegCreateKeyExW, RegDeleteTreeW,
    RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegSetValueExW,
};
use windows::core::{HSTRING, Owned, PCWSTR, PWSTR};

/// One registry value: name (UTF-16, may be empty for the default value), type and data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegValue {
    pub name: Vec<u16>,
    pub kind: u32,
    pub data: Vec<u8>,
}

impl RegValue {
    /// A `REG_SZ` value.
    pub fn string(name: &str, value: &str) -> RegValue {
        RegValue {
            name: name.encode_utf16().collect(),
            kind: REG_SZ.0,
            data: value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect(),
        }
    }

    /// A `REG_DWORD` value.
    pub fn dword(name: &str, value: u32) -> RegValue {
        RegValue {
            name: name.encode_utf16().collect(),
            kind: REG_DWORD.0,
            data: value.to_le_bytes().to_vec(),
        }
    }
}

/// The values of a key at one point in time. Subkeys are not included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySnapshot {
    /// `None` when the key did not exist.
    pub values: Option<Vec<RegValue>>,
}

/// The registry root a key lives under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

impl Hive {
    fn root(self) -> HKEY {
        match self {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }
}

/// Reads all values of `<hive>\<subkey>`.
pub fn snapshot_key(hive: Hive, subkey: &str) -> io::Result<KeySnapshot> {
    let mut key = HKEY::default();
    // SAFETY: on success the opened key is owned and closed on drop.
    let status = unsafe {
        RegOpenKeyExW(
            hive.root(),
            &HSTRING::from(subkey),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(KeySnapshot { values: None });
    }
    status.ok()?;
    // SAFETY: opened above.
    let key = unsafe { Owned::new(key) };
    Ok(KeySnapshot {
        values: Some(read_values(*key)?),
    })
}

/// Makes `<hive>\<subkey>` hold exactly the snapshot's values: changed values are
/// rewritten, added ones deleted, removed ones recreated. A key that did not exist is
/// deleted.
pub fn restore_key(hive: Hive, subkey: &str, snapshot: &KeySnapshot) -> io::Result<()> {
    let Some(values) = &snapshot.values else {
        // SAFETY: plain call with a NUL-terminated subkey name.
        let status = unsafe { RegDeleteTreeW(hive.root(), &HSTRING::from(subkey)) };
        return if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            status.ok().map_err(io::Error::from)
        };
    };
    let mut key = HKEY::default();
    // SAFETY: on success the key is owned and closed on drop.
    unsafe {
        RegCreateKeyExW(
            hive.root(),
            &HSTRING::from(subkey),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    }
    .ok()?;
    // SAFETY: opened above.
    let key = unsafe { Owned::new(key) };
    for current in read_values(*key)? {
        if !values.iter().any(|v| v.name == current.name) {
            let name = nul_terminated(&current.name);
            // SAFETY: `name` is NUL-terminated and outlives the call.
            unsafe { RegDeleteValueW(*key, PCWSTR(name.as_ptr())) }.ok()?;
        }
    }
    for value in values {
        let name = nul_terminated(&value.name);
        // SAFETY: `name` is NUL-terminated; the data slice carries its own length.
        unsafe {
            RegSetValueExW(
                *key,
                PCWSTR(name.as_ptr()),
                None,
                REG_VALUE_TYPE(value.kind),
                Some(&value.data),
            )
        }
        .ok()?;
    }
    Ok(())
}

fn read_values(key: HKEY) -> io::Result<Vec<RegValue>> {
    let (mut count, mut max_name, mut max_data) = (0u32, 0u32, 0u32);
    // SAFETY: only the requested counters are written.
    unsafe {
        RegQueryInfoKeyW(
            key,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&mut count),
            Some(&mut max_name),
            Some(&mut max_data),
            None,
            None,
        )
    }
    .ok()?;
    let mut values = Vec::with_capacity(count as usize);
    for index in 0..count {
        // Lengths: the name in characters plus its NUL, the data in bytes.
        let mut name = vec![0u16; max_name as usize + 1];
        let mut name_len = name.len() as u32;
        let mut data = vec![0u8; max_data as usize];
        let mut data_len = data.len() as u32;
        let mut kind = 0u32;
        // SAFETY: the buffers are as large as the lengths passed with them.
        unsafe {
            RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            )
        }
        .ok()?;
        name.truncate(name_len as usize);
        data.truncate(data_len as usize);
        values.push(RegValue { name, kind, data });
    }
    Ok(values)
}

fn nul_terminated(name: &[u16]) -> Vec<u16> {
    name.iter().copied().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_BINARY, REG_DWORD, REG_OPTION_NON_VOLATILE,
        REG_SZ, REG_VALUE_TYPE, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegSetValueExW,
    };
    use windows::core::{HSTRING, Owned, PCWSTR};

    /// A throwaway key under HKCU\Software, deleted on drop.
    struct TestKey(String);

    impl TestKey {
        fn new(name: &str) -> Self {
            TestKey(format!(
                r"Software\procmon-mcp-test-{name}-{}",
                std::process::id()
            ))
        }

        fn set(&self, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) {
            let mut key = HKEY::default();
            unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(self.0.as_str()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &mut key,
                    None,
                )
                .ok()
                .unwrap();
                let key = Owned::new(key);
                RegSetValueExW(*key, &HSTRING::from(name), None, kind, Some(data))
                    .ok()
                    .unwrap();
            }
        }

        fn delete_value(&self, name: &str) {
            let mut key = HKEY::default();
            unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(self.0.as_str()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    None,
                    &mut key,
                    None,
                )
                .ok()
                .unwrap();
                let key = Owned::new(key);
                RegDeleteValueW(*key, &HSTRING::from(name)).ok().unwrap();
            }
        }
    }

    impl Drop for TestKey {
        fn drop(&mut self) {
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(self.0.as_str()));
            }
        }
    }

    fn sorted(mut snapshot: KeySnapshot) -> KeySnapshot {
        if let Some(values) = &mut snapshot.values {
            values.sort_by(|a, b| a.name.cmp(&b.name));
        }
        snapshot
    }

    fn sz(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    #[test]
    fn string_values_are_nul_terminated_utf16() {
        let v = RegValue::string("DisplayName", "procmon-mcp");
        assert_eq!(v.name, "DisplayName".encode_utf16().collect::<Vec<_>>());
        assert_eq!(v.kind, REG_SZ.0);
        assert_eq!(v.data, sz("procmon-mcp"));
    }

    #[test]
    fn dword_values_are_four_little_endian_bytes() {
        let v = RegValue::dword("NoModify", 1);
        assert_eq!(v.kind, REG_DWORD.0);
        assert_eq!(v.data, [1, 0, 0, 0]);
    }

    #[test]
    fn restores_changed_added_and_removed_values() {
        let key = TestKey::new("restore");
        key.set("FilterRules", REG_BINARY, &[1, 2, 3]);
        key.set("DestructiveFilter", REG_DWORD, &0u32.to_le_bytes());
        key.set("Logfile", REG_SZ, &sz(""));
        key.set("", REG_SZ, &sz("default value"));
        let before = snapshot_key(Hive::CurrentUser, &key.0).unwrap();
        assert_eq!(before.values.as_ref().unwrap().len(), 4);

        // What Procmon does on exit: rewrite, add and (hypothetically) remove values.
        key.set("FilterRules", REG_BINARY, &[9, 9]);
        key.set("DestructiveFilter", REG_DWORD, &1u32.to_le_bytes());
        key.set("NewSetting", REG_DWORD, &7u32.to_le_bytes());
        key.delete_value("Logfile");

        restore_key(Hive::CurrentUser, &key.0, &before).unwrap();
        // Value order carries no meaning; a recreated value comes back last.
        assert_eq!(
            sorted(snapshot_key(Hive::CurrentUser, &key.0).unwrap()),
            sorted(before)
        );
    }

    #[test]
    fn deletes_a_key_that_did_not_exist_before() {
        let key = TestKey::new("absent");
        let before = snapshot_key(Hive::CurrentUser, &key.0).unwrap();
        assert_eq!(before.values, None);
        key.set("Created", REG_DWORD, &1u32.to_le_bytes());
        restore_key(Hive::CurrentUser, &key.0, &before).unwrap();
        assert_eq!(
            snapshot_key(Hive::CurrentUser, &key.0).unwrap().values,
            None
        );
    }

    #[test]
    fn restoring_an_absent_key_that_is_still_absent_succeeds() {
        let key = TestKey::new("still-absent");
        let before = snapshot_key(Hive::CurrentUser, &key.0).unwrap();
        assert!(restore_key(Hive::CurrentUser, &key.0, &before).is_ok());
    }
}
