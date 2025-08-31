use std::collections::HashMap;
use std::any::Any;
use std::io::{BufWriter, Write};

#[derive(Debug, Clone)]
pub struct DataPoint<T> {
    pub timestamp: f64,
    pub value: T,
}

impl<T> DataPoint<T> {
    pub fn new(timestamp: f64, value: T) -> Self {
        Self { timestamp, value }
    }
}

#[derive(Debug)]
pub struct DataStore {
    data: HashMap<String, Box<dyn Any>>,
}

impl DataStore {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
    
    pub fn push<T: 'static>(&mut self, key: &str, timestamp: f64, value: T) {
        let entry = self.data.entry(key.to_string()).or_insert_with(|| {
            Box::new(Vec::<DataPoint<T>>::new())
        });
        
        if let Some(vec) = entry.downcast_mut::<Vec<DataPoint<T>>>() {
            vec.push(DataPoint::new(timestamp, value));
        } else {
            panic!("Type mismatch for key: {}", key);
        }
    }
    
    pub fn signal_count(&self) -> usize {
        self.data.len()
    }
    
    // Convenience methods for common types
    pub fn push_int(&mut self, key: &str, timestamp: f64, value: i64) {
        self.push(key, timestamp, value);
    }

    pub fn push_uint(&mut self, key: &str, timestamp: f64, value: u64) {
        self.push(key, timestamp, value);
    }
    
    #[allow(dead_code)]
    pub fn push_float(&mut self, key: &str, timestamp: f64, value: f64) {
        self.push(key, timestamp, value);
    }

    #[allow(dead_code)]
    pub fn push_string(&mut self, key: &str, timestamp: f64, value: String) {
        self.push(key, timestamp, value);
    }

    fn sort_by_timestamp(&mut self) {
        // Sort all vectors by timestamp in ascending order
        for (_, data) in &mut self.data {
            if let Some(vec) = data.downcast_mut::<Vec<DataPoint<i64>>>() {
                vec.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap_or(std::cmp::Ordering::Equal));
            } else if let Some(vec) = data.downcast_mut::<Vec<DataPoint<u64>>>() {
                vec.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap_or(std::cmp::Ordering::Equal));
            } else if let Some(vec) = data.downcast_mut::<Vec<DataPoint<f64>>>() {
                vec.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap_or(std::cmp::Ordering::Equal));
            } else if let Some(vec) = data.downcast_mut::<Vec<DataPoint<String>>>() {
                vec.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap_or(std::cmp::Ordering::Equal));
            }
        }
    }

    pub fn write_to_stream<W: Write>(&mut self, writer: W) -> Result<(), Box<dyn std::error::Error>> {
        self.sort_by_timestamp();
        
        // Use a large buffer for batched writes (1MB buffer)
        let mut buf_writer = BufWriter::with_capacity(1024 * 1024, writer);
        
        // Write magic header to identify binary format
        buf_writer.write_all(b"BLF2MDF\x01")?; // 8 bytes: magic + version
        
        // Write signal count as 4-byte little-endian
        buf_writer.write_all(&(self.data.len() as u32).to_le_bytes())?;
        
        for (key, data) in &self.data {
            // Write signal name length and name
            let key_bytes = key.as_bytes();
            buf_writer.write_all(&(key_bytes.len() as u16).to_le_bytes())?;
            buf_writer.write_all(key_bytes)?;
            
            // Determine type and write data
            if let Some(vec) = data.downcast_ref::<Vec<DataPoint<i64>>>() {
                buf_writer.write_all(&[1u8])?; // Type marker: 1 = i64
                buf_writer.write_all(&(vec.len() as u32).to_le_bytes())?;
                
                // Batch write all data points for this signal
                let mut batch = Vec::with_capacity(vec.len() * 16); // 16 bytes per point
                for point in vec {
                    batch.extend_from_slice(&point.timestamp.to_le_bytes()); // 8 bytes
                    batch.extend_from_slice(&point.value.to_le_bytes());     // 8 bytes
                }
                buf_writer.write_all(&batch)?;
                
            } else if let Some(vec) = data.downcast_ref::<Vec<DataPoint<u64>>>() {
                buf_writer.write_all(&[2u8])?; // Type marker: 2 = u64
                buf_writer.write_all(&(vec.len() as u32).to_le_bytes())?;
                
                // Batch write all data points for this signal
                let mut batch = Vec::with_capacity(vec.len() * 16); // 16 bytes per point
                for point in vec {
                    batch.extend_from_slice(&point.timestamp.to_le_bytes()); // 8 bytes
                    batch.extend_from_slice(&point.value.to_le_bytes());     // 8 bytes
                }
                buf_writer.write_all(&batch)?;
                
            } else if let Some(vec) = data.downcast_ref::<Vec<DataPoint<f64>>>() {
                buf_writer.write_all(&[3u8])?; // Type marker: 3 = f64
                buf_writer.write_all(&(vec.len() as u32).to_le_bytes())?;
                
                // Batch write all data points for this signal
                let mut batch = Vec::with_capacity(vec.len() * 16); // 16 bytes per point
                for point in vec {
                    batch.extend_from_slice(&point.timestamp.to_le_bytes()); // 8 bytes
                    batch.extend_from_slice(&point.value.to_le_bytes());     // 8 bytes
                }
                buf_writer.write_all(&batch)?;
                
            } else if let Some(vec) = data.downcast_ref::<Vec<DataPoint<String>>>() {
                buf_writer.write_all(&[4u8])?; // Type marker: 4 = string
                buf_writer.write_all(&(vec.len() as u32).to_le_bytes())?;
                
                // Strings can't be batched as easily due to variable length
                // But we can still use the buffered writer for better performance
                for point in vec {
                    buf_writer.write_all(&point.timestamp.to_le_bytes())?; // 8 bytes
                    let value_bytes = point.value.as_bytes();
                    buf_writer.write_all(&(value_bytes.len() as u16).to_le_bytes())?; // 2 bytes
                    buf_writer.write_all(value_bytes)?;
                }
            }
        }
        
        buf_writer.flush()?;
        Ok(())
    }
}

impl Default for DataStore {
    fn default() -> Self {
        Self::new()
    }
}