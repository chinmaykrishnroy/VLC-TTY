//! Versioned, bounded PCM framing. The wire sample format is always S16LE.
use std::io::{self, Read, Write};

pub const MAGIC: &[u8; 8] = b"VTTYPCM1";
pub const RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
pub const MAX_SAMPLES: usize = 1920;
pub const MAX_PACKET: usize = 4096;

#[derive(Debug, PartialEq)]
pub enum Message {
    Format,
    Samples {
        generation: u64,
        pts: i64,
        lead_us: i64,
        data: Vec<i16>,
    },
    State {
        generation: u64,
        paused: bool,
    },
    End,
    Telemetry {
        generation: u64,
        queued_frames: u32,
        underruns: u64,
        pts: i64,
    },
}

pub fn write(w: &mut impl Write, message: &Message) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(MAX_PACKET);
    match message {
        Message::Format => {
            bytes.push(1);
            bytes.extend(RATE.to_le_bytes());
            bytes.extend(CHANNELS.to_le_bytes());
        }
        Message::Samples {
            generation,
            pts,
            lead_us,
            data,
        } => {
            if data.is_empty() || data.len() > MAX_SAMPLES || data.len() % 2 != 0 {
                return Err(invalid("invalid sample count"));
            }
            bytes.push(2);
            bytes.extend(generation.to_le_bytes());
            bytes.extend(pts.to_le_bytes());
            bytes.extend(lead_us.to_le_bytes());
            for sample in data {
                bytes.extend(sample.to_le_bytes());
            }
        }
        Message::State { generation, paused } => {
            bytes.push(3);
            bytes.extend(generation.to_le_bytes());
            bytes.push(u8::from(*paused));
        }
        Message::End => bytes.push(4),
        Message::Telemetry {
            generation,
            queued_frames,
            underruns,
            pts,
        } => {
            bytes.push(5);
            bytes.extend(generation.to_le_bytes());
            bytes.extend(queued_frames.to_le_bytes());
            bytes.extend(underruns.to_le_bytes());
            bytes.extend(pts.to_le_bytes());
        }
    }
    w.write_all(&(bytes.len() as u32).to_le_bytes())?;
    w.write_all(&bytes)
}

pub fn read(r: &mut impl Read) -> io::Result<Message> {
    let mut length = [0; 4];
    r.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_PACKET {
        return Err(invalid("audio packet exceeds limit"));
    }
    let mut bytes = vec![0; length];
    r.read_exact(&mut bytes)?;
    let u64_at = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
    match bytes[0] {
        1 if length == 7
            && bytes[1..5] == RATE.to_le_bytes()
            && bytes[5..7] == CHANNELS.to_le_bytes() =>
        {
            Ok(Message::Format)
        }
        2 if length > 25 && (length - 25).is_multiple_of(4) && (length - 25) / 2 <= MAX_SAMPLES => {
            Ok(Message::Samples {
                generation: u64_at(1),
                pts: u64_at(9) as i64,
                lead_us: u64_at(17) as i64,
                data: bytes[25..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| i16::from_le_bytes([s[0], s[1]]))
                    .collect(),
            })
        }
        3 if length == 10 && bytes[9] <= 1 => Ok(Message::State {
            generation: u64_at(1),
            paused: bytes[9] != 0,
        }),
        4 if length == 1 => Ok(Message::End),
        5 if length == 29 => Ok(Message::Telemetry {
            generation: u64_at(1),
            queued_frames: u32::from_le_bytes(bytes[9..13].try_into().unwrap()),
            underruns: u64_at(13),
            pts: u64_at(21) as i64,
        }),
        _ => Err(invalid("invalid audio message or unsupported PCM format")),
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn authenticate_client(stream: &mut (impl Read + Write), token: &[u8; 32]) -> io::Result<()> {
    stream.write_all(MAGIC)?;
    stream.write_all(token)?;
    let mut ack = [0; 8];
    stream.read_exact(&mut ack)?;
    if &ack != MAGIC {
        return Err(invalid("audio authentication failed"));
    }
    Ok(())
}
#[cfg(any(feature = "audio-client", test))]
pub fn authenticate_server(stream: &mut (impl Read + Write), token: &[u8; 32]) -> io::Result<()> {
    let mut hello = [0; 40];
    stream.read_exact(&mut hello)?;
    let mut difference = 0u8;
    for (a, b) in hello[..8].iter().zip(MAGIC) {
        difference |= a ^ b;
    }
    for (a, b) in hello[8..].iter().zip(token) {
        difference |= a ^ b;
    }
    if difference != 0 {
        return Err(invalid("audio authentication failed"));
    }
    stream.write_all(MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_framing_rejects_truncated_oversized_and_unknown_packets() {
        for bytes in [
            vec![0, 0, 0, 0],
            vec![1, 16, 0, 0],
            vec![1, 0, 0, 0, 99],
            vec![10, 0, 0, 0, 3, 0],
        ] {
            assert!(read(&mut &bytes[..]).is_err());
        }
        for message in [
            Message::Format,
            Message::End,
            Message::State {
                generation: 9,
                paused: true,
            },
            Message::Samples {
                generation: 10,
                pts: -1,
                lead_us: 100,
                data: vec![i16::MIN, i16::MAX],
            },
            Message::Telemetry {
                generation: 2,
                queued_frames: 40,
                underruns: 3,
                pts: 72,
            },
        ] {
            let mut bytes = Vec::new();
            write(&mut bytes, &message).unwrap();
            assert_eq!(read(&mut &bytes[..]).unwrap(), message);
        }
    }
}
