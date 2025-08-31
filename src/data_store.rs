use std::collections::HashMap;
use std::any::Any;

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
    
    pub fn get<T: 'static>(&self, key: &str) -> Option<&Vec<DataPoint<T>>> {
        self.data.get(key)?.downcast_ref::<Vec<DataPoint<T>>>()
    }
    
    pub fn get_mut<T: 'static>(&mut self, key: &str) -> Option<&mut Vec<DataPoint<T>>> {
        self.data.get_mut(key)?.downcast_mut::<Vec<DataPoint<T>>>()
    }
    
    pub fn len(&self, key: &str) -> usize {
        // This is tricky with Any - we'll try common types
        if let Some(vec) = self.data.get(key) {
            if let Some(v) = vec.downcast_ref::<Vec<DataPoint<i64>>>() {
                return v.len();
            }
            if let Some(v) = vec.downcast_ref::<Vec<DataPoint<u64>>>() {
                return v.len();
            }
            if let Some(v) = vec.downcast_ref::<Vec<DataPoint<f64>>>() {
                return v.len();
            }
            if let Some(v) = vec.downcast_ref::<Vec<DataPoint<String>>>() {
                return v.len();
            }
        }
        0
    }

    pub fn signal_count(&self) -> usize {
        self.data.len()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }
    
    pub fn keys(&self) -> Vec<&String> {
        self.data.keys().collect()
    }
    
    // Convenience methods for common types
    pub fn push_int(&mut self, key: &str, timestamp: f64, value: i64) {
        self.push(key, timestamp, value);
    }

    pub fn push_uint(&mut self, key: &str, timestamp: f64, value: u64) {
        self.push(key, timestamp, value);
    }
    
    pub fn push_float(&mut self, key: &str, timestamp: f64, value: f64) {
        self.push(key, timestamp, value);
    }
    
    pub fn push_string(&mut self, key: &str, timestamp: f64, value: String) {
        self.push(key, timestamp, value);
    }
    
    pub fn get_ints(&self, key: &str) -> Option<&Vec<DataPoint<i64>>> {
        self.get::<i64>(key)
    }
    
    pub fn get_uints(&self, key: &str) -> Option<&Vec<DataPoint<u64>>> {
        self.get::<u64>(key)
    }

    pub fn get_floats(&self, key: &str) -> Option<&Vec<DataPoint<f64>>> {
        self.get::<f64>(key)
    }
    
    pub fn get_strings(&self, key: &str) -> Option<&Vec<DataPoint<String>>> {
        self.get::<String>(key)
    }
}

impl Default for DataStore {
    fn default() -> Self {
        Self::new()
    }
}