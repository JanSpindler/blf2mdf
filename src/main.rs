use can_dbc::{Message, DBC, SignalExtendedValueType, ValueType, ByteOrder};
use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::Stdio;
use tqdm::tqdm;
use std::collections::HashMap;
use rfd::FileDialog;
use std::env;

mod blf_reader;
use blf_reader::BlfReader;

mod data_store;
use data_store::DataStore;

const PYTHON_CODE: &str = include_str!("../script/write_mdf.py");

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

    // Init data store
    let mut data_store = DataStore::new();

    // Create message ID to DBC message map for each bus
    // Create message ID to DBC map for each bus
    // Add all signal units to data store
    let mut dbc_messages_maps = Vec::<HashMap<u32, &can_dbc::Message>>::new();
    let mut dbc_map: Vec<HashMap<u32, &DBC>> = Vec::<HashMap<u32, &DBC>>::new();
    let mut signal_bus_map: HashMap<String, u32> = HashMap::new();
    for (bus_idx, bus_dbcs) in dbcs.iter().enumerate() {
        let mut bus_dbc_messages_map = HashMap::<u32, &can_dbc::Message>::new();
        let mut bus_dbc_map = HashMap::<u32, &DBC>::new();

        // Iterate over all dbcs for this bus
        for dbc in bus_dbcs {
            // Iterate over all messages in dbc
            for msg in dbc.messages() {
                let msg_id = msg.message_id().raw();
                bus_dbc_messages_map.insert(msg_id, msg);
                bus_dbc_map.insert(msg_id, dbc);

                // Iterate over all signals in this message
                for sig in msg.signals() {
                    data_store.set_unit(sig.name(), sig.unit());
                    
                    if !signal_bus_map.contains_key(sig.name()) {
                        signal_bus_map.insert(sig.name().clone(), bus_idx as u32);

                        match dbc.value_descriptions_for_signal(*msg.message_id(), sig.name()) {
                            Some(value_table) => {
                                let mut table = HashMap::<i64, String>::new();
                                for val_desc in value_table {
                                    table.insert(*val_desc.a() as i64, val_desc.b().clone());
                                }
                                data_store.set_value_table(sig.name(), table);
                            },
                            None => {}
                        }
                    }
                }
            }
        }
        
        dbc_messages_maps.push(bus_dbc_messages_map);
        dbc_map.push(bus_dbc_map);
    }

    // Init signal to bus map to avoid duplicates

    // Start reading BLF file
    let mut reader = match BlfReader::new(&blf_file) {
        Ok(reader) => reader,
        Err(e) => {
            eprintln!("Error opening BLF file {file_path}: {}", e);
            return;
        }
    };

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
        if bus_idx >= dbcs.len() {
            continue 'message_loop;
        }
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

            // Skip if float
            if is_float {
                continue 'signal_loop;
            }

            // Get signal info
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
    // Get number of busses
    println!("Enter the number of CAN busses: ");
    let mut num_busses = String::new();
    io::stdin().read_line(&mut num_busses).unwrap();
    let num_busses: usize = num_busses.trim().parse().unwrap();
    if num_busses == 0 {
        println!("Number of CAN busses must be greater than 0");
        return;
    }

    // Get blf files
    let blf_files = FileDialog::new()
        .set_title("Select .blf files")
        .set_directory(env::current_dir().unwrap())
        .add_filter("BLF Files", &["blf"])
        .pick_files()
        .unwrap();
    let blf_folder = blf_files[0].parent().unwrap();

    let mut dbcs = Vec::<Vec<DBC>>::new();
    let mut num_total_dbcs = 0;
    for bus_idx in 0..num_busses {
        let dbc_files = match FileDialog::new()
            .set_title(format!("Select .dbc files for bus {}", bus_idx + 1))
            .set_directory(&blf_folder)
            .add_filter("DBC Files", &["dbc"])
            .pick_files() {
            Some(files) => files,
            None => Vec::<PathBuf>::new()
        };

        let mut bus_dbcs = Vec::<DBC>::new();
        for dbc_file in dbc_files {
            let dbc = load_dbc(dbc_file.as_path().to_str().unwrap()).unwrap();
            bus_dbcs.push(dbc);
        }
        num_total_dbcs += bus_dbcs.len();
        dbcs.push(bus_dbcs);
    }

    if num_total_dbcs == 0 {
        println!("No .dbc files loaded");
        return;
    }

    for entry in blf_files {
        let path = entry.as_path();
        process_file(path.with_extension("").to_str().unwrap(), &dbcs);
    }
}
