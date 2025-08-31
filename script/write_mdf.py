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
