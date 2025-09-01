use can_dbc::{Message, DBC, SignalExtendedValueType, ValueType, ByteOrder};
use std::fs::{read_dir, File};
use std::io::{self, Read};
use std::process::Stdio;
use tqdm::tqdm;
use std::collections::HashMap;
use rfd::FileDialog;
use std::env;

mod blf_reader;
use blf_reader::BlfReader;

mod data_store;
use data_store::DataStore;

const PYTHON_CODE: &str = r#"
import logging
logging.getLogger('canmatrix.formats').setLevel(logging.ERROR)

import asammdf
import sys
import tqdm
import numpy as np
import struct
from io import BufferedReader


def load_from_stdin():
    """
    Optimized binary reading with larger buffers and timestamp handling.
    """
    # Use buffered reader for better performance
    reader = BufferedReader(sys.stdin.buffer, buffer_size=1024*1024)  # 1MB buffer
    
    # Read magic header
    magic = reader.read(8)
    if magic != b"BLF2MDF\x01":
        raise ValueError("Invalid binary format")
    
    # Read signal count
    signal_count = struct.unpack('<I', reader.read(4))[0]
    
    for _ in range(signal_count):
        # Read signal name
        name_len = struct.unpack('<H', reader.read(2))[0]
        signal_name = reader.read(name_len).decode('utf-8')
        
        # Read type marker
        type_marker = struct.unpack('B', reader.read(1))[0]
        
        # Read data count
        data_count = struct.unpack('<I', reader.read(4))[0]
        
        last_timestep = None
        
        # Read all data at once for this signal for better performance
        if type_marker in [1, 2, 3]:  # i64, u64, f64 - all have 16 bytes per data point
            # Read entire signal data in one operation
            data_bytes = reader.read(data_count * 16)  # 16 bytes per data point
            
            # Use numpy for fast unpacking
            data_array = np.frombuffer(data_bytes, dtype=np.uint8)
            data_array = data_array.reshape(-1, 16)
            
            # Extract timestamps (first 8 bytes of each row)
            timestamps_raw = data_array[:, :8].view(np.float64).flatten()
            
            # Apply timestamp handling (ensure monotonic increasing)
            timestamps = []
            for timestamp in timestamps_raw:
                if last_timestep is not None and timestamp <= last_timestep:
                    timestamp = last_timestep + 1e-9
                last_timestep = timestamp
                timestamps.append(timestamp)
            
            timestamps = np.array(timestamps)
            
            # Extract values based on type
            if type_marker == 1:  # i64
                values = data_array[:, 8:].view(np.int64).flatten()
            elif type_marker == 2:  # u64
                values = data_array[:, 8:].view(np.uint64).flatten()
            elif type_marker == 3:  # f64
                values = data_array[:, 8:].view(np.float64).flatten()
            
            yield signal_name, timestamps, values
            
        elif type_marker == 4:  # string - variable length, can't batch as easily
            timestamps = []
            values = []
            
            for _ in range(data_count):
                timestamp = struct.unpack('<d', reader.read(8))[0]
                if last_timestep is not None and timestamp <= last_timestep:
                    timestamp = last_timestep + 1e-9
                last_timestep = timestamp
                
                value_len = struct.unpack('<H', reader.read(2))[0]
                value = reader.read(value_len).decode('utf-8')
                timestamps.append(timestamp)
                values.append(value)
            
            yield signal_name, np.array(timestamps), np.array(values)
        
        else:
            # Unknown type marker - skip this signal
            print(f"Warning: Unknown type marker {type_marker} for signal {signal_name}")
            continue


if __name__ == '__main__':
    # Get output file path
    if len(sys.argv) < 2:
        print("Usage: python write_mdf.py <output.mf4>")
        sys.exit(1)
    output_file = sys.argv[1]

    # Create a new MDF file
    mdf = asammdf.MDF()

    # Process signals from stdin
    for signal_name, timestamps, values in tqdm.tqdm(load_from_stdin()):
        mdf.append(asammdf.Signal(
            samples=values,
            timestamps=timestamps,
            name=signal_name
        ))

    # Save to MF4 file
    mdf.save(output_file, overwrite=True, compression=2)
    mdf.close()
    print(f"Finished writing MDF file to {output_file}")
"#;

fn load_dbc(path_str: &str) -> Result<DBC, Box<dyn std::error::Error>> {
    // Read file
    let mut file = File::open(path_str)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;

    // Read file as dbc
    let content = String::from_utf8_lossy(&buffer);
    let dbc = DBC::from_slice(content.as_bytes())
        .map_err(|e| format!("{:?}", e)
    )?;

    // Return
    Ok(dbc)
}

fn extract_signal_raw(
        data: &[u8], 
        start_bit: i64, 
        bit_count: i64, 
        is_big_endian: bool) -> Option<u64> {
    if bit_count == 0 || bit_count > 64 {
        return None;
    }
    
    let total_bits = data.len() as i64 * 8;
    if start_bit >= total_bits {
        return None;
    }
    
    let mut result = 0u64;
    
    if is_big_endian {
        // Motorola byte order (MSB first)
        // Start bit is the MSB of the signal
        if start_bit < bit_count - 1 {
            return None;
        }
        
        for bit_index in 0..bit_count {
            let absolute_bit = start_bit - bit_index;
            if absolute_bit >= total_bits {
                continue;
            }
            
            let byte_index = (absolute_bit / 8) as usize;
            let bit_in_byte = absolute_bit % 8;
            
            if byte_index < data.len() {
                let bit_value = (data[byte_index] >> bit_in_byte) & 1;
                if bit_value != 0 {
                    result |= 1u64 << bit_index;
                }
            }
        }
    } else {
        // Intel byte order (LSB first)
        // Start bit is the LSB of the signal
        for bit_index in 0..bit_count {
            let absolute_bit = start_bit + bit_index;
            if absolute_bit >= total_bits {
                break;
            }
            
            let byte_index = (absolute_bit / 8) as usize;
            let bit_in_byte = absolute_bit % 8;
            
            if byte_index < data.len() {
                let bit_value = (data[byte_index] >> bit_in_byte) & 1;
                if bit_value != 0 {
                    result |= 1u64 << bit_index;
                }
            }
        }
    }
    
    Some(result)
}

fn process_signal(
    msg_data: &[u8],
    start_bit: i64,
    bit_count: i64,
    is_big_endian: bool,
    is_signed: bool,
    store_as_float: bool,
    factor: f64,
    offset: f64,
    signal_name: &str,
    msg_timestamp: f64,
    data_store: &mut DataStore,
) {
    let raw_value = match extract_signal_raw(msg_data, start_bit, bit_count, is_big_endian) {
        Some(v) => v,
        None => {
            // println!("Failed to extract signal {} from message ID {}", signal_name, msg.arbitration_id);
            return;
        }
    };

    if is_signed {
        // Convert raw value to signed using two's complement
        let signed_value = if bit_count < 64 {
            // Create mask for the number of bits
            let mask = (1u64 << bit_count) - 1;
            let masked_value = raw_value & mask;

            // Check if sign bit is set
            let sign_bit = 1u64 << (bit_count - 1);
            if masked_value & sign_bit != 0 {
                // Negative value - extend sign bits
                let sign_extension = !((1u64 << bit_count) - 1);
                (masked_value | sign_extension) as i64
            } else {
                // Positive value
                masked_value as i64
            }
        } else {
            raw_value as i64
        };

        if store_as_float {
            let physical_value = (signed_value as f64) * factor + offset;
            data_store.push_float(signal_name, msg_timestamp, physical_value);
        } else {
            let physical_value = (factor as i64) * signed_value + (offset as i64);
            data_store.push_int(signal_name, msg_timestamp, physical_value);
        }
    } else {
        if store_as_float {
            let physical_value = (raw_value as f64) * factor + offset;
            data_store.push_float(signal_name, msg_timestamp, physical_value);
        } else {
            let physical_value = (factor as u64) * raw_value + (offset as u64);
            data_store.push_uint(signal_name, msg_timestamp, physical_value);
        }
    }
}

fn process_file(file_path: &str, dbcs: &[Vec<DBC>]) {
    // File names
    let blf_file = file_path.to_owned() + ".blf";
    let output_file = file_path.to_owned() + ".mf4";

    // Create message ID to DBC message map for each bus
    let mut dbc_messages_maps = Vec::<HashMap<u32, &can_dbc::Message>>::new();
    let mut dbc_map: Vec<HashMap<u32, &DBC>> = Vec::<HashMap<u32, &DBC>>::new();
    for bus_dbcs in dbcs {
        let mut bus_dbc_messages_map = HashMap::<u32, &can_dbc::Message>::new();
        let mut bus_dbc_map = HashMap::<u32, &DBC>::new();
        
        for dbc in bus_dbcs {
            for msg in dbc.messages() {
                let msg_id = msg.message_id().raw();
                bus_dbc_messages_map.insert(msg_id, msg);
                bus_dbc_map.insert(msg_id, dbc);
            }
        }
        
        dbc_messages_maps.push(bus_dbc_messages_map);
        dbc_map.push(bus_dbc_map);
    }

    // Init signal to bus map to avoid duplicates
    let signal_bus_map: HashMap<String, u32> = HashMap::new();

    // Start reading BLF file
    let mut reader = match BlfReader::new(&blf_file) {
        Ok(reader) => reader,
        Err(e) => {
            eprintln!("Error opening BLF file {file_path}: {}", e);
            return;
        }
    };

    // Init data store
    let mut data_store = DataStore::new();

    // Init first timestamp
    let mut first_timestamp = f64::MAX;

    // Iterate over all messages in blf file
    println!("Reading BLF file: {}", &blf_file);
    'message_loop: for msg_result in tqdm(reader.messages()) {
        // Get raw can message
        let msg = match msg_result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("Error reading message: {}", e);
                continue 'message_loop;
            }
        };

        // Get bus DBCs
        let bus_idx = msg.channel as usize;
        let bus_dbcs = &dbcs[bus_idx];
        let msg_id = msg.arbitration_id;

        // Get timestamp
        let mut msg_timestamp = msg.timestamp;
        if first_timestamp == f64::MAX {
            first_timestamp = msg_timestamp;
        }
        msg_timestamp -= first_timestamp;

        // Get dbc for message
        let dbc_msg: &Message = match dbc_messages_maps[bus_idx].get(&msg_id) {
            Some(msg) => msg,
            None => continue 'message_loop
        };

        // Get message raw data
        let msg_data = msg.data;

        // Get mux signal
        let dbc = dbc_map[bus_idx].get(&msg_id).unwrap();
        let mux_signal = dbc.message_multiplexor_switch(*dbc_msg.message_id());
        let current_mux_value = match mux_signal {
            Ok(Some(mux_signal)) => {
                let start_bit = *mux_signal.start_bit() as i64;
                let bit_count = *mux_signal.signal_size() as i64;
                let is_big_endian = *mux_signal.byte_order() == ByteOrder::BigEndian;
                match extract_signal_raw(
                        &msg_data, start_bit, bit_count, is_big_endian) {
                    Some(v) => v,
                    None => {
                        // println!("Failed to extract mux signal {} from message ID {}", mux_signal.name(), msg.arbitration_id);
                        continue 'message_loop;
                    }
                }
            },
            Ok(None) => 0,
            Err(_) => {
                continue 'message_loop;
            }
        };

        // Iterate over all signals in message
        'signal_loop: for signal in dbc_msg.signals() {
            // Check if we want to skip because signal name already found on another bus
            if let Some(signal_bus_idx) = signal_bus_map.get(signal.name()) {
                if bus_idx != *signal_bus_idx as usize {
                    continue 'signal_loop;
                }
            }

            // Check if float or signed
            let mut is_float = false;
            for dbc in bus_dbcs {
                if let Some(v) = dbc.extended_value_type_for_signal(
                        *dbc_msg.message_id(), signal.name()) {
                    if *v != SignalExtendedValueType::SignedOrUnsignedInteger {
                        is_float = true;
                        break;
                    }
                }
            }
            let is_signed = *signal.value_type() == ValueType::Signed;

            if is_float {
                continue 'signal_loop;
            }

            let start_bit = *signal.start_bit() as i64;
            let bit_count = *signal.signal_size() as i64;
            let is_big_endian = *signal.byte_order() == ByteOrder::BigEndian;

            let factor = *signal.factor();
            let offset = *signal.offset();
            let store_as_float = is_float || (factor.fract() != 0.0) || (offset.fract() != 0.0);

            match signal.multiplexer_indicator() {
                can_dbc::MultiplexIndicator::Plain | can_dbc::MultiplexIndicator::Multiplexor => {
                    process_signal(
                        &msg_data, start_bit, bit_count, is_big_endian, is_signed, store_as_float, 
                        factor, offset, signal.name(), msg_timestamp, &mut data_store);
                },
                can_dbc::MultiplexIndicator::MultiplexedSignal(mux_idx) => {
                    if *mux_idx == current_mux_value {
                        process_signal(
                            &msg_data, start_bit, bit_count, is_big_endian, is_signed, store_as_float, 
                            factor, offset, signal.name(), msg_timestamp, &mut data_store);
                    }
                },
                mux_ind => {
                    println!("Can't handle MultiplexIndicator {:?}", mux_ind);
                    continue 'signal_loop;
                }
            }
        };
    }

    println!("{} signals found", data_store.signal_count());

    let mut child = std::process::Command::new("python")
        .arg("-c")
        .arg(PYTHON_CODE)
        .arg(&output_file)
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to spawn python process");

    if let Some(stdin) = child.stdin.take() {
        data_store.write_to_stream(stdin).unwrap();
    }
    child.wait().expect("Failed to wait on child process");
}

fn main() {
    println!("Enter the number of CAN busses: ");
    let mut num_busses = String::new();
    io::stdin().read_line(&mut num_busses).unwrap();
    let num_busses: usize = num_busses.trim().parse().unwrap();
    if num_busses == 0 {
        println!("Number of CAN busses must be greater than 0");
        return;
    }

    let blf_folder = FileDialog::new()
        .set_directory(env::current_dir().unwrap())
        .pick_folder()
        .unwrap();

    let mut dbcs = Vec::<Vec<DBC>>::new();
    for _ in 0..num_busses {
        let dbc_files = FileDialog::new()
            .set_directory(&blf_folder)
            .add_filter("DBC Files", &["dbc"])
            .pick_files()
            .unwrap();

        let mut bus_dbcs = Vec::<DBC>::new();
        for dbc_file in dbc_files {
            let dbc = load_dbc(dbc_file.as_path().to_str().unwrap()).unwrap();
            bus_dbcs.push(dbc);
        }
        dbcs.push(bus_dbcs);
    }

    let entries = read_dir(&blf_folder).expect("Failed to read directory");
    for entry in entries {
        let path = entry.expect("Failed to get entry").path();
        if !path.is_file() {
            continue;
        }

        if path.extension().is_none() {
            continue;
        }

        if path.extension().unwrap() != "blf" {
            continue;
        }

        process_file(path.with_extension("").to_str().unwrap(), &dbcs);
    }
}
