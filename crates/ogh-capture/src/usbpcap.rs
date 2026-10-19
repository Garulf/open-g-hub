use std::fmt;

const PCAP_MAGIC_LE: u32 = 0xa1b2c3d4;
const LINKTYPE_USBPCAP: u32 = 249;
const GLOBAL_HEADER_LEN: usize = 24;
const RECORD_HEADER_LEN: usize = 16;
const USBPCAP_MIN_HEADER_LEN: usize = 27;
const INFO_OFFSET: usize = 16;
const TRANSFER_OFFSET: usize = 22;
const DATA_LENGTH_OFFSET: usize = 23;
const TRANSFER_INTERRUPT: u8 = 1;
const INFO_DEVICE_TO_HOST: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out,
    In,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transfer {
    pub t_ms: f64,
    pub dir: Direction,
    pub data: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PcapError {
    TooShort,
    BadMagic(u32),
    LinkType(u32),
    BadUsbpcapHeader { record: usize },
}

impl fmt::Display for PcapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "file is shorter than a pcap global header"),
            Self::BadMagic(magic) => write!(
                f,
                "unsupported pcap magic {magic:#010x} (only little-endian microsecond pcap, 0xa1b2c3d4, is supported)"
            ),
            Self::LinkType(linktype) => write!(
                f,
                "unexpected link type {linktype} (USBPcap captures use {LINKTYPE_USBPCAP})"
            ),
            Self::BadUsbpcapHeader { record } => {
                write!(f, "record {record} has a malformed USBPcap header")
            }
        }
    }
}

impl std::error::Error for PcapError {}

pub fn read_hid_transfers(bytes: &[u8]) -> Result<Vec<Transfer>, PcapError> {
    let mut rest = read_global_header(bytes)?;
    let mut first_timestamp_us = None;
    let mut transfers = Vec::new();

    for index in 0.. {
        let Some((header, body, remainder)) = split_record(rest) else {
            break;
        };
        rest = remainder;

        let origin_us = *first_timestamp_us.get_or_insert(header.timestamp_us);
        if let Some(transfer) = interrupt_transfer(body, header.timestamp_us - origin_us, index)? {
            transfers.push(transfer);
        }
    }
    Ok(transfers)
}

fn read_global_header(bytes: &[u8]) -> Result<&[u8], PcapError> {
    if bytes.len() < GLOBAL_HEADER_LEN {
        return Err(PcapError::TooShort);
    }
    let magic = u32_le(bytes, 0);
    if magic != PCAP_MAGIC_LE {
        return Err(PcapError::BadMagic(magic));
    }
    let linktype = u32_le(bytes, 20);
    if linktype != LINKTYPE_USBPCAP {
        return Err(PcapError::LinkType(linktype));
    }
    Ok(&bytes[GLOBAL_HEADER_LEN..])
}

struct RecordHeader {
    timestamp_us: i64,
}

/// Returns `None` at end of input, including when the final record is cut short.
fn split_record(bytes: &[u8]) -> Option<(RecordHeader, &[u8], &[u8])> {
    let header = bytes.get(..RECORD_HEADER_LEN)?;
    let incl_len = usize::try_from(u32_le(header, 8)).ok()?;
    let body_end = RECORD_HEADER_LEN.checked_add(incl_len)?;
    let body = bytes.get(RECORD_HEADER_LEN..body_end)?;
    let timestamp_us = i64::from(u32_le(header, 0)) * 1_000_000 + i64::from(u32_le(header, 4));
    Some((RecordHeader { timestamp_us }, body, &bytes[body_end..]))
}

fn interrupt_transfer(
    packet: &[u8],
    elapsed_us: i64,
    record: usize,
) -> Result<Option<Transfer>, PcapError> {
    let malformed = || PcapError::BadUsbpcapHeader { record };
    if packet.len() < USBPCAP_MIN_HEADER_LEN {
        return Err(malformed());
    }
    if packet[TRANSFER_OFFSET] != TRANSFER_INTERRUPT {
        return Ok(None);
    }

    let header_len = usize::from(u16::from_le_bytes([packet[0], packet[1]]));
    let payload = packet.get(header_len..).ok_or_else(malformed)?;
    let declared_len = usize::try_from(u32_le(packet, DATA_LENGTH_OFFSET)).unwrap_or(usize::MAX);
    let data = &payload[..payload.len().min(declared_len)];
    if data.is_empty() {
        return Ok(None);
    }

    let dir = if packet[INFO_OFFSET] & INFO_DEVICE_TO_HOST != 0 {
        Direction::In
    } else {
        Direction::Out
    };
    Ok(Some(Transfer {
        t_ms: elapsed_us as f64 / 1000.0,
        dir,
        data: data.to_vec(),
    }))
}

fn u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
