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

fn process_file(file_path: &str, dbcs: &[Vec<DBC>]) {
    let blf_file = file_path.to_owned() + ".blf";
    let output_file = file_path.to_owned() + ".mf4";

    let mut dbc_messages_maps = Vec::<HashMap<u32, &can_dbc::Message>>::new();
    for bus_dbcs in dbcs {
        dbc_messages_maps.push(
            bus_dbcs.iter()
            .flat_map(|dbc| dbc.messages())
            .map(|msg| (msg.message_id().raw(), msg))
            .collect()
        );
    }

    let signal_bus_map: HashMap<String, u32> = HashMap::new();

    let mut reader = match BlfReader::new(&blf_file) {
        Ok(reader) => reader,
        Err(e) => {
            eprintln!("Error opening BLF file {file_path}: {}", e);
            return;
        }
    };

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

        let bus_idx = msg.channel as usize;
        let bus_dbcs = &dbcs[bus_idx];

        let mut msg_timestamp = msg.timestamp;
        if first_timestamp == f64::MAX {
            first_timestamp = msg_timestamp;
        }
        msg_timestamp -= first_timestamp;

        let found_dbc_msg: &Message = match dbc_messages_maps[bus_idx].get(&msg.arbitration_id) {
            Some(msg) => msg,
            None => continue 'message_loop
        };

        let msg_data = msg.data;

        'signal_loop: for signal in found_dbc_msg.signals() {
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
                        *found_dbc_msg.message_id(), signal.name()) {
                    if *v != SignalExtendedValueType::SignedOrUnsignedInteger {
                        is_float = true;
                        break;
                    }
                }
            }
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
