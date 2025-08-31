use can_dbc::DBC;
use std::{fs::File, io};
use std::io::Read;

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

fn main() {
    // Get file path from user
    println!("Please enter the DBC file path:");
    let mut path = String::new();
    match io::stdin().read_line(&mut path) {
        Ok(_) => path = path.trim().to_owned(),
        Err(e) => {
            eprintln!("Failed to read file path: {}", e);
            return;
        }
    }

    // Read file
    let dbc = match load_dbc(&path.to_string()) {
        Ok(dbc) => {
            println!("Loaded DBC file with {} messages", dbc.messages().len());
            dbc
        },
        Err(e) => {
            eprintln!("Error loading DBC file: {}", e);
            return;
        }
    };
}
