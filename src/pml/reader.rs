use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use super::bytes::{ByteReader, read_vec, seek_to};
use super::event::{Event, EventClass};
use super::tables::{Process, read_processes, read_strings};
use super::{Header, PmlError};

/// Size of the fixed part of each event.
const EVENT_HEADER_SIZE: u64 = 0x34;

/// Reads a PML file. Tables are loaded up front; events are read one at a time.
pub struct PmlReader<R> {
    inner: R,
    header: Header,
    processes: HashMap<u32, Process>,
    event_offsets: Vec<u64>,
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
        let event_offsets = read_event_offsets(&mut inner, &header)?;
        Ok(PmlReader {
            inner,
            header,
            processes,
            event_offsets,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn processes(&self) -> impl Iterator<Item = &Process> {
        self.processes.values()
    }

    pub fn process(&self, index: u32) -> Option<&Process> {
        self.processes.get(&index)
    }

    /// Iterates over events in file order.
    pub fn events(&mut self) -> impl Iterator<Item = Result<Event, PmlError>> + '_ {
        let offsets = std::mem::take(&mut self.event_offsets);
        let inner = &mut self.inner;
        offsets.into_iter().map(move |off| read_event(inner, off))
    }
}

/// Each entry is a `u32` absolute file offset followed by one flags byte.
fn read_event_offsets<R: Read + Seek>(r: &mut R, header: &Header) -> Result<Vec<u64>, PmlError> {
    seek_to(r, header.events_offsets_array_offset)?;
    let raw = read_vec(r, u64::from(header.number_of_events) * 5)?;
    Ok(raw
        .chunks_exact(5)
        .map(|e| u64::from(u32::from_le_bytes([e[0], e[1], e[2], e[3]])))
        .collect())
}

fn read_event<R: Read + Seek>(r: &mut R, offset: u64) -> Result<Event, PmlError> {
    seek_to(r, offset)?;
    let buf = read_vec(r, EVENT_HEADER_SIZE)?;
    let mut h = ByteReader::new(&buf);
    let process_index = h.u32()?;
    let tid = h.u32()?;
    let class = EventClass::from_u32(h.u32()?);
    let operation = h.u16()?;
    h.seek(0x14)?;
    let duration = h.u64()?;
    let timestamp = h.u64()?;
    let result = h.u32()?;
    Ok(Event {
        process_index,
        tid,
        class,
        operation,
        duration,
        timestamp,
        result,
    })
}
