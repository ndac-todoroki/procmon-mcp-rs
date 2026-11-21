use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use super::bytes::{ByteReader, read_vec, seek_to};
use super::details::{self, DetailInput};
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

    /// Calls `f` for each event in file order, together with its process.
    pub fn for_each_event(
        &mut self,
        mut f: impl FnMut(&Event, Option<&Process>),
    ) -> Result<(), PmlError> {
        let is_64bit = self.header.is_64bit;
        for &off in &self.event_offsets {
            let event = read_event(&mut self.inner, off, is_64bit)?;
            f(&event, self.processes.get(&event.process_index));
        }
        Ok(())
    }

    /// Iterates over events in file order.
    pub fn events(&mut self) -> impl Iterator<Item = Result<Event, PmlError>> + '_ {
        let offsets = std::mem::take(&mut self.event_offsets);
        let is_64bit = self.header.is_64bit;
        let inner = &mut self.inner;
        offsets
            .into_iter()
            .map(move |off| read_event(inner, off, is_64bit))
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

fn read_event<R: Read + Seek>(r: &mut R, offset: u64, is_64bit: bool) -> Result<Event, PmlError> {
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
    let stack_depth = u64::from(h.u16()?);
    h.skip(2)?;
    let details_size = u64::from(h.u32()?);
    let extra_offset = u64::from(h.u32()?);

    let pointer_size = if is_64bit { 8 } else { 4 };
    seek_to(r, offset + EVENT_HEADER_SIZE + stack_depth * pointer_size)?;
    let details = read_vec(r, details_size)?;
    // The extra block's offset is relative to the event start and prefixed by its size.
    let extra = if extra_offset == 0 {
        None
    } else {
        seek_to(r, offset + extra_offset)?;
        let size = read_vec(r, 2)?;
        Some(read_vec(
            r,
            u64::from(u16::from_le_bytes([size[0], size[1]])),
        )?)
    };
    let (path, detail) = details::parse(&DetailInput {
        class,
        operation,
        is_64bit,
        details: &details,
        extra: extra.as_deref(),
    });
    Ok(Event {
        process_index,
        tid,
        class,
        operation,
        duration,
        timestamp,
        result,
        path,
        detail,
    })
}
