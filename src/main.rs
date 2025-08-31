use can_dbc::{Message, DBC, SignalExtendedValueType, ValueType, ByteOrder};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;

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
        let end_bit = if start_bit >= bit_count - 1 {
            start_bit - bit_count + 1
        } else {
            return None;
        };
        
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
    let dbc = load_dbc("./data/DBC/GXe_CAN1.dbc").unwrap();
    let dbc_messages = dbc.messages();

    let mut reader = BlfReader::new("./data/Measurement_32.blf").unwrap();
    let messages = reader.read_messages().unwrap();
    
    let mut message_ids = HashSet::<u32>::new();
    let mut data_store = DataStore::new();

    'message_loop: for msg in messages {
        let msg_bus = msg.channel;
        if msg_bus != 0 {
            // println!("Skipping bus {}", msg_bus);
            continue 'message_loop;
        }
        
        message_ids.insert(msg.arbitration_id);

        let mut found_dbc_msg: Option<&Message> = None;
        'dbc_message_loop: for dbc_msg in dbc_messages {
            if dbc_msg.message_id().raw() == msg.arbitration_id {
                found_dbc_msg = Some(dbc_msg);
                break 'dbc_message_loop;
            }
        }
        if found_dbc_msg.is_none() {
            continue 'message_loop;
        }
        let found_dbc_msg = found_dbc_msg.unwrap();

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
                        data_store.push_int(signal.name(), msg.timestamp, raw_value as i64);
                    } else {
                        data_store.push_uint(signal.name(), msg.timestamp, raw_value);
                    }
                },
                can_dbc::MultiplexIndicator::MultiplexedSignal(mux_idx) => {
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

    println!("{} messages and {} signals found", message_ids.len(), data_store.signal_count());
}
