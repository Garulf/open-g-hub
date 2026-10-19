use ogh_capture::usbpcap::{Direction, PcapError, read_hid_transfers};

const LINKTYPE_USBPCAP: u32 = 249;
const TRANSFER_INTERRUPT: u8 = 1;
const TRANSFER_CONTROL: u8 = 2;
const TRANSFER_ISOCHRONOUS: u8 = 0;
const USBPCAP_HEADER_LEN: u16 = 27;

fn global_header(linktype: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0xa1b2c3d4u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&65535u32.to_le_bytes());
    out.extend_from_slice(&linktype.to_le_bytes());
    out
}

fn usbpcap_packet(transfer: u8, device_to_host: bool, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&USBPCAP_HEADER_LEN.to_le_bytes());
    out.extend_from_slice(&1u64.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&9u16.to_le_bytes());
    out.push(u8::from(device_to_host));
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.push(if device_to_host { 0x84 } else { 0x04 });
    out.push(transfer);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn record_header(ts_sec: u32, ts_usec: u32, incl_len: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&ts_sec.to_le_bytes());
    out.extend_from_slice(&ts_usec.to_le_bytes());
    out.extend_from_slice(&incl_len.to_le_bytes());
    out.extend_from_slice(&incl_len.to_le_bytes());
    out
}

fn record(ts_sec: u32, ts_usec: u32, packet: &[u8]) -> Vec<u8> {
    let mut out = record_header(ts_sec, ts_usec, packet.len() as u32);
    out.extend_from_slice(packet);
    out
}

const SAMPLE_OUT: [u8; 7] = [0x02, 0x0c, 0x05, 0x00, 0x0a, 0x1c, 0x01];
const SAMPLE_IN: [u8; 4] = [0x02, 0x0c, 0x05, 0x01];

fn sample_capture() -> Vec<u8> {
    let mut pcap = global_header(LINKTYPE_USBPCAP);
    pcap.extend(record(
        100,
        0,
        &usbpcap_packet(TRANSFER_CONTROL, true, &[0x80, 0x06]),
    ));
    pcap.extend(record(
        100,
        12_500,
        &usbpcap_packet(TRANSFER_INTERRUPT, false, &SAMPLE_OUT),
    ));
    pcap.extend(record(
        100,
        20_000,
        &usbpcap_packet(TRANSFER_INTERRUPT, true, &SAMPLE_IN),
    ));
    pcap
}

#[test]
fn rejects_non_usbpcap_linktype() {
    let pcap = global_header(1);
    assert_eq!(read_hid_transfers(&pcap), Err(PcapError::LinkType(1)));
}

#[test]
fn rejects_unknown_magic() {
    let mut pcap = global_header(LINKTYPE_USBPCAP);
    pcap[..4].copy_from_slice(&0xa1b2c3d5u32.to_le_bytes());
    assert_eq!(
        read_hid_transfers(&pcap),
        Err(PcapError::BadMagic(0xa1b2c3d5))
    );
}

#[test]
fn rejects_input_shorter_than_global_header() {
    assert_eq!(read_hid_transfers(&[0xd4, 0xc3]), Err(PcapError::TooShort));
}

#[test]
fn extracts_interrupt_in_and_out() {
    let transfers = read_hid_transfers(&sample_capture()).unwrap();

    assert_eq!(transfers.len(), 2);
    assert_eq!(transfers[0].dir, Direction::Out);
    assert_eq!(transfers[0].data, SAMPLE_OUT);
    assert_eq!(transfers[1].dir, Direction::In);
    assert_eq!(transfers[1].data, SAMPLE_IN);
}

#[test]
fn timestamps_are_relative_to_first_record_of_any_type() {
    let transfers = read_hid_transfers(&sample_capture()).unwrap();

    assert_eq!(transfers[0].t_ms, 12.5);
    assert_eq!(transfers[1].t_ms, 20.0);
}

#[test]
fn skips_empty_interrupt_completions() {
    let mut pcap = global_header(LINKTYPE_USBPCAP);
    pcap.extend(record(
        0,
        0,
        &usbpcap_packet(TRANSFER_INTERRUPT, false, &SAMPLE_OUT),
    ));
    pcap.extend(record(
        0,
        1_000,
        &usbpcap_packet(TRANSFER_INTERRUPT, false, &[]),
    ));

    let transfers = read_hid_transfers(&pcap).unwrap();

    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers[0].data, SAMPLE_OUT);
}

#[test]
fn skips_isochronous_transfers() {
    let mut pcap = global_header(LINKTYPE_USBPCAP);
    pcap.extend(record(
        0,
        0,
        &usbpcap_packet(TRANSFER_ISOCHRONOUS, true, &[0u8; 192]),
    ));

    assert_eq!(read_hid_transfers(&pcap).unwrap(), vec![]);
}

#[test]
fn tolerates_truncated_final_record() {
    let complete = sample_capture();
    let extra = record(
        100,
        30_000,
        &usbpcap_packet(TRANSFER_INTERRUPT, true, &SAMPLE_IN),
    );

    for cut in 0..extra.len() {
        let mut pcap = complete.clone();
        pcap.extend_from_slice(&extra[..cut]);

        let transfers = read_hid_transfers(&pcap)
            .unwrap_or_else(|e| panic!("cut at {cut} bytes of final record failed: {e}"));
        assert_eq!(transfers.len(), 2, "cut at {cut} bytes");
    }
}
