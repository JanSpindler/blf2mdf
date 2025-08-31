use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use anyhow::{anyhow, Result};
use flate2::read::ZlibDecoder;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CanMessage {
    pub timestamp: f64,
    pub arbitration_id: u32,
    pub is_extended_id: bool,
    pub is_remote_frame: bool,
    pub is_rx: bool,
    pub is_fd: bool,
    pub is_error_frame: bool,
    pub dlc: u8,
    pub data: Vec<u8>,
    pub channel: u8,
    pub bitrate_switch: bool,
    pub error_state_indicator: bool,
}

// Constants - matching Python implementation exactly
const LOG_CONTAINER: u32 = 10;
const CAN_MESSAGE: u32 = 1;
const CAN_MESSAGE2: u32 = 86;
// const CAN_FD_MESSAGE: u32 = 100;
// const CAN_FD_MESSAGE_64: u32 = 101;
const CAN_ERROR_EXT: u32 = 73;

const NO_COMPRESSION: u16 = 0;
const ZLIB_DEFLATE: u16 = 2;

const CAN_MSG_EXT: u32 = 0x80000000;
const REMOTE_FLAG: u8 = 0x80;
const DIR: u8 = 0x1;

const TIME_TEN_MICS: u32 = 0x00000001;
// const TIME_ONE_NANS: u32 = 0x00000002;

const TIME_TEN_MICS_FACTOR: f64 = 1e-5;
const TIME_ONE_NANS_FACTOR: f64 = 1e-9;

pub struct BlfReader<R: Read + Seek> {
    reader: BufReader<R>,
    start_timestamp: f64,
    tail: Vec<u8>,
    pos: usize,
}

impl BlfReader<File> {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        Self::from_reader(file)
    }
}

impl<R: Read + Seek> BlfReader<R> {
    pub fn from_reader(reader: R) -> Result<Self> {
        let mut buf_reader = BufReader::new(reader);
        
        // Read file header - first part to get header size
        let mut header_start = [0u8; 8];
        buf_reader.read_exact(&mut header_start)?;
        
        // Check signature
        if &header_start[0..4] != b"LOGG" {
            return Err(anyhow!("Unexpected file format"));
        }
        
        let header_size = u32::from_le_bytes([header_start[4], header_start[5], header_start[6], header_start[7]]);
        
        // Read the full header
        let mut full_header = vec![0u8; header_size as usize];
        buf_reader.seek(SeekFrom::Start(0))?;
        buf_reader.read_exact(&mut full_header)?;
        
        // Extract start timestamp from header (at offset 56 for SYSTEMTIME)
        let start_timestamp = if full_header.len() >= 72 {
            systemtime_to_timestamp(&full_header[56..72])
        } else {
            0.0
        };
                
        Ok(BlfReader {
            reader: buf_reader,
            start_timestamp,
            tail: Vec::new(),
            pos: 0,
        })
    }
    
    pub fn read_messages(&mut self) -> Result<Vec<CanMessage>> {
        let mut all_messages = Vec::new();
        
        // Main loop - exactly like Python's __iter__ method
        loop {
            // Read object header base (16 bytes) - OBJ_HEADER_BASE_STRUCT
            let mut obj_header_data = [0u8; 16];
            match self.reader.read_exact(&mut obj_header_data) {
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break;
                },
                Err(e) => return Err(e.into()),
            }
            
            // Parse object header base
            let signature = &obj_header_data[0..4];
            if signature != b"LOBJ" {
                return Err(anyhow!("Invalid object signature: {:?}", signature));
            }
            
            let obj_size = u32::from_le_bytes([obj_header_data[8], obj_header_data[9], obj_header_data[10], obj_header_data[11]]);
            let obj_type = u32::from_le_bytes([obj_header_data[12], obj_header_data[13], obj_header_data[14], obj_header_data[15]]);
                        
            // Read the object data (size - 16 bytes we already read)
            let obj_data_size = obj_size - 16;
            let mut obj_data = vec![0u8; obj_data_size as usize];
            self.reader.read_exact(&mut obj_data)?;
            
            // Read padding bytes
            let padding = obj_size % 4;
            if padding > 0 {
                let mut pad_buf = vec![0u8; padding as usize];
                self.reader.read_exact(&mut pad_buf)?;
            }
            
            // Only process LOG_CONTAINER objects - this is the key insight!
            if obj_type == LOG_CONTAINER {
                // Parse LOG_CONTAINER_STRUCT: compression method (2 bytes) + 6 padding + uncompressed_size (4 bytes) + 4 padding
                if obj_data.len() < 16 {
                    println!("Container data too short");
                    continue;
                }
                
                let compression_method = u16::from_le_bytes([obj_data[0], obj_data[1]]);
                                
                // Get container data (skip the 16-byte LOG_CONTAINER header)
                let container_data = &obj_data[16..];
                
                // Decompress based on method
                let decompressed_data = match compression_method {
                    NO_COMPRESSION => {
                        container_data.to_vec()
                    },
                    ZLIB_DEFLATE => {
                        let mut decoder = ZlibDecoder::new(container_data);
                        let mut decompressed = Vec::new();
                        match decoder.read_to_end(&mut decompressed) {
                            Ok(_) => {
                                decompressed
                            },
                            Err(e) => {
                                println!("  Decompression failed: {}", e);
                                continue;
                            }
                        }
                    },
                    _ => {
                        println!("  Unknown compression method: {}", compression_method);
                        continue;
                    }
                };
                
                // Parse the decompressed container data
                let messages = self.parse_container_data(&decompressed_data)?;
                all_messages.extend(messages);
            } else {
                println!("  Skipping non-container object type: {}", obj_type);
            }
        }
        
        Ok(all_messages)
    }
    
    fn parse_container_data(&mut self, data: &[u8]) -> Result<Vec<CanMessage>> {
        // Combine with tail from previous container
        let full_data = if !self.tail.is_empty() {
            let mut combined = self.tail.clone();
            combined.extend_from_slice(data);
            self.tail.clear(); // Clear the tail after using it
            combined
        } else {
            data.to_vec()
        };
        
        let mut messages = Vec::new();
        let mut pos = 0;
        let max_pos = full_data.len();
        
        // Parse objects within the container - this follows Python's _parse_data method
        while pos + 16 <= max_pos {
            self.pos = pos;
            
            // Find next LOBJ signature
            let lobj_pos = match find_pattern(&full_data[pos..std::cmp::min(pos + 8, max_pos)], b"LOBJ") {
                Some(offset) => pos + offset,
                None => {
                    if pos + 8 > max_pos {
                        break; // Not enough data
                    }
                    pos += 1;
                    continue;
                }
            };
            
            pos = lobj_pos;
            
            if pos + 16 > max_pos {
                break; // Not enough data for header
            }
            
            // Parse object header
            let signature = &full_data[pos..pos + 4];
            if signature != b"LOBJ" {
                pos += 1;
                continue;
            }
            
            let header_version = u16::from_le_bytes([full_data[pos + 6], full_data[pos + 7]]);
            let obj_size = u32::from_le_bytes([full_data[pos + 8], full_data[pos + 9], full_data[pos + 10], full_data[pos + 11]]);
            let obj_type = u32::from_le_bytes([full_data[pos + 12], full_data[pos + 13], full_data[pos + 14], full_data[pos + 15]]);
            
            let next_pos = pos + obj_size as usize;
            if next_pos > max_pos {
                break; // Object continues in next container
            }
            
            pos += 16; // Skip base header
            
            // Parse extended header based on version
            let timestamp = match header_version {
                1 => {
                    if pos + 16 > max_pos { break; }
                    let flags = u32::from_le_bytes([full_data[pos], full_data[pos + 1], full_data[pos + 2], full_data[pos + 3]]);
                    let ts = u64::from_le_bytes([
                        full_data[pos + 8], full_data[pos + 9], full_data[pos + 10], full_data[pos + 11],
                        full_data[pos + 12], full_data[pos + 13], full_data[pos + 14], full_data[pos + 15]
                    ]);
                    pos += 16;
                    
                    let factor = if flags == TIME_TEN_MICS { TIME_TEN_MICS_FACTOR } else { TIME_ONE_NANS_FACTOR };
                    (ts as f64 * factor) + self.start_timestamp
                },
                2 => {
                    if pos + 16 > max_pos { break; }
                    let flags = u32::from_le_bytes([full_data[pos], full_data[pos + 1], full_data[pos + 2], full_data[pos + 3]]);
                    let ts = u64::from_le_bytes([
                        full_data[pos + 8], full_data[pos + 9], full_data[pos + 10], full_data[pos + 11],
                        full_data[pos + 12], full_data[pos + 13], full_data[pos + 14], full_data[pos + 15]
                    ]);
                    pos += 16;
                    
                    let factor = if flags == TIME_TEN_MICS { TIME_TEN_MICS_FACTOR } else { TIME_ONE_NANS_FACTOR };
                    (ts as f64 * factor) + self.start_timestamp
                },
                _ => {
                    pos = next_pos;
                    continue;
                }
            };
            
            // Parse message data based on type
            if let Ok(Some(msg)) = self.parse_message_by_type(obj_type, &full_data[pos..next_pos], timestamp) {
                messages.push(msg);
            }
            
            pos = next_pos;
        }
        
        // Save remaining data for next container - be more precise about what to save
        let remaining_data = &full_data[self.pos..];
        if remaining_data.len() > 0 {
            self.tail = remaining_data.to_vec();
        } else {
            self.tail.clear();
        }

        Ok(messages)
    }
    
    fn parse_message_by_type(&self, obj_type: u32, data: &[u8], timestamp: f64) -> Result<Option<CanMessage>> {
        match obj_type {
            CAN_MESSAGE | CAN_MESSAGE2 => {
                // Python: CAN_MSG_STRUCT = struct.Struct("<HBBL8s")
                // channel (H=2), flags (B=1), dlc (B=1), arbitration_id (L=4), data (8s=8)
                if data.len() < 16 {
                    return Ok(None);
                }
                
                let channel = u16::from_le_bytes([data[0], data[1]]);
                let flags = data[2];
                let dlc = data[3];
                let can_id = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                
                // Python takes data[:dlc] from the 8-byte data field
                let data_start = 8;
                let data_end = std::cmp::min(data_start + dlc as usize, std::cmp::min(data_start + 8, data.len()));
                let msg_data = data[data_start..data_end].to_vec();
                
                Ok(Some(CanMessage {
                    timestamp,
                    arbitration_id: can_id & 0x1FFFFFFF,
                    is_extended_id: (can_id & CAN_MSG_EXT) != 0,
                    is_remote_frame: (flags & REMOTE_FLAG) != 0,
                    is_rx: (flags & DIR) == 0,  // Python: is_rx=not bool(flags & DIR)
                    is_fd: false,
                    is_error_frame: false,
                    dlc,
                    data: msg_data,
                    channel: if channel > 0 { (channel - 1) as u8 } else { 0 }, // Python: channel - 1
                    bitrate_switch: false,
                    error_state_indicator: false,
                }))
            },
            CAN_ERROR_EXT => {
                // Python: CAN_ERROR_EXT_STRUCT = struct.Struct("<HHLBBBxLLH2x8s")
                // channel (H), length (H), flags (L), ecc (B), position (B), dlc (B), x, 
                // frame_length (L), id (L), flags_ext (H), 2x, data (8s)
                if data.len() < 26 { // 2+2+4+1+1+1+1+4+4+2+2+8 = 32, but let's be safe
                    return Ok(None);
                }
                
                let channel = u16::from_le_bytes([data[0], data[1]]);
                let dlc = data[5]; // position 5 in the struct
                let can_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]); // position of 'id' field
                
                // Data field starts after all the fixed fields
                let data_start = 26; // Adjust based on actual struct layout
                let data_end = std::cmp::min(data_start + dlc as usize, data.len());
                let msg_data = if data_start < data.len() {
                    data[data_start..data_end].to_vec()
                } else {
                    Vec::new()
                };
                
                Ok(Some(CanMessage {
                    timestamp,
                    arbitration_id: can_id & 0x1FFFFFFF,
                    is_extended_id: (can_id & CAN_MSG_EXT) != 0,
                    is_remote_frame: false,
                    is_rx: true,
                    is_fd: false,
                    is_error_frame: true,
                    dlc,
                    data: msg_data,
                    channel: if channel > 0 { (channel - 1) as u8 } else { 0 },
                    bitrate_switch: false,
                    error_state_indicator: false,
                }))
            },
            // ... rest of the match arms
            _ => Ok(None),
        }
    }
}

fn find_pattern(data: &[u8], pattern: &[u8]) -> Option<usize> {
    data.windows(pattern.len()).position(|window| window == pattern)
}

fn systemtime_to_timestamp(data: &[u8]) -> f64 {
    if data.len() < 16 {
        return 0.0;
    }
    
    // SYSTEMTIME structure: year, month, dayofweek, day, hour, minute, second, milliseconds
    let year = u16::from_le_bytes([data[0], data[1]]) as i32;
    let month = u16::from_le_bytes([data[2], data[3]]) as u32;
    let day = u16::from_le_bytes([data[6], data[7]]) as u32;
    let hour = u16::from_le_bytes([data[8], data[9]]) as u32;
    let minute = u16::from_le_bytes([data[10], data[11]]) as u32;
    let second = u16::from_le_bytes([data[12], data[13]]) as u32;
    let millisecond = u16::from_le_bytes([data[14], data[15]]) as u32;
    
    // Convert to Unix timestamp (simplified)
    let days_since_1970 = (year - 1970) * 365 + (year - 1969) / 4; // Rough approximation
    let timestamp = days_since_1970 as f64 * 86400.0 + 
                   (month - 1) as f64 * 30.0 * 86400.0 + 
                   (day - 1) as f64 * 86400.0 + 
                   hour as f64 * 3600.0 + 
                   minute as f64 * 60.0 + 
                   second as f64 + 
                   millisecond as f64 / 1000.0;
    
    timestamp
}

// fn dlc2len(dlc: u8) -> u8 {
//     match dlc {
//         0..=8 => dlc,
//         9 => 12,
//         10 => 16,
//         11 => 20,
//         12 => 24,
//         13 => 32,
//         14 => 48,
//         15 => 64,
//         _ => 8,
//     }
// }
