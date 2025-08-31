use can_dbc::{Message, DBC, SignalExtendedValueType, ValueType, ByteOrder};
use std::fs::File;
use std::io::Read;
use std::process::Stdio;
use tqdm::tqdm;
use std::collections::HashMap;

mod blf_reader;
use blf_reader::BlfReader;

mod data_store;
use data_store::DataStore;

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

fn extract_signal_with_byte_order(
        data: &Vec<u8>, 
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

fn main() {
    const FILE: &str = "./data/Measurement_40";
    let blf_file = FILE.to_owned() + ".blf";
    let output_file = FILE.to_owned() + ".mf4";

    let dbc = load_dbc("./data/DBC/GXe_CAN1.dbc").unwrap();
    let dbc_messages_map: HashMap<u32, &can_dbc::Message> = dbc.messages()
        .iter()
        .map(|msg| (msg.message_id().raw(), msg))
        .collect();

    let mut reader = BlfReader::new(&blf_file).unwrap();
    let mut data_store = DataStore::new();
    let mut first_timestamp = f64::MAX;

    println!("Reading BLF file: {}", &blf_file);
    'message_loop: for msg_result in tqdm(reader.messages()) {
        let msg = match msg_result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("Error reading message: {}", e);
                continue 'message_loop;
            }
        };

        let msg_bus = msg.channel;
        if msg_bus != 0 {
            continue 'message_loop;
        }

        let mut msg_timestamp = msg.timestamp;
        if first_timestamp == f64::MAX {
            first_timestamp = msg_timestamp;
        }
        msg_timestamp -= first_timestamp;

        let found_dbc_msg: &Message = match dbc_messages_map.get(&msg.arbitration_id) {
            Some(msg) => msg,
            None => continue 'message_loop
        };

        let msg_data = msg.data;

        'signal_loop: for signal in found_dbc_msg.signals() {
            let is_float = match dbc.extended_value_type_for_signal(
                    *found_dbc_msg.message_id(), signal.name()) {
                Some(v) => *v != SignalExtendedValueType::SignedOrUnsignedInteger,
                None => false,
            };
            
            let is_signed = *signal.value_type() == ValueType::Signed;

            if is_float {
                // println!("Float signals not implemented yet: {}", signal.name());
                continue 'signal_loop;
            }

            if is_signed {
                // println!("Signed signals not implemented yet: {}", signal.name());
                continue 'signal_loop;
            }

            let start_bit = *signal.start_bit() as i64;
            let bit_count = *signal.signal_size() as i64;
            let is_big_endian = *signal.byte_order() == ByteOrder::BigEndian;

            match signal.multiplexer_indicator() {
                can_dbc::MultiplexIndicator::Plain => {
                    let raw_value = match extract_signal_with_byte_order(
                            &msg_data, start_bit, bit_count, is_big_endian) {
                        Some(v) => v,
                        None => {
                            // println!("Failed to extract signal {} from message ID {}", signal.name(), msg.arbitration_id);
                            continue 'signal_loop;
                        }
                    };

                    if is_signed {
                        data_store.push_int(signal.name(), msg_timestamp, raw_value as i64);
                    } else {
                        data_store.push_uint(signal.name(), msg_timestamp, raw_value);
                    }
                },
                can_dbc::MultiplexIndicator::MultiplexedSignal(_) => {
                },
                can_dbc::MultiplexIndicator::Multiplexor => {

                },
                mux_ind => {
                    println!("Can't handle MultiplexIndicator {:?}", mux_ind);
                    continue 'signal_loop;
                }
            }
        }
    }

    println!("{} signals found", data_store.signal_count());
    
    let mut child = std::process::Command::new("python")
    .arg("script/write_mdf.py")
    .arg(&output_file)
    .stdin(Stdio::piped())
    .spawn()
    .expect("Failed to spawn python write_mdf.py");

    if let Some(stdin) = child.stdin.take() {
        data_store.write_to_stream(stdin).unwrap();
    }
    child.wait().expect("Failed to wait on child process");
}
