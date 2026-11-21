use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use super::bytes::{read_vec, seek_to};
use super::tables::{Process, read_processes, read_strings};
use super::{Header, PmlError};

/// Reads a PML file. Tables are loaded up front; events are read one at a time.
pub struct PmlReader<R> {
    inner: R,
    header: Header,
    processes: HashMap<u32, Process>,
}

impl PmlReader<BufReader<File>> {
    pub fn open(path: &Path) -> Result<Self, PmlError> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> PmlReader<R> {
    pub fn new(mut inner: R) -> Result<Self, PmlError> {
        seek_to(&mut inner, 0)?;
        let header = Header::parse(&read_vec(&mut inner, Header::SIZE as u64)?)?;
        let strings = read_strings(&mut inner, header.strings_table_offset)?;
        let processes = read_processes(&mut inner, header.process_table_offset, &strings)?;
        Ok(PmlReader {
            inner,
            header,
            processes,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn processes(&self) -> impl Iterator<Item = &Process> {
        self.processes.values()
    }
}
