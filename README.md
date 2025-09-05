# blf2mdf

A Rust-based tool for converting CAN bus data from BLF (Binary Logging Format) files to MF4 (Measurement Data Format 4) files, with signal interpretation using DBC (Database CAN) files.

## Overview

This tool reads CAN bus messages from BLF files, interprets the binary data using DBC database files to extract meaningful signals, and outputs the data in MF4 format for analysis in tools like CANape, ATI Vision, or asammdf.

## Features

- **Multi-file processing**: Load and process multiple BLF files simultaneously
- **Multi-bus support**: Handle multiple CAN buses with separate DBC configurations
- **Signal interpretation**: Convert raw CAN data to physical values using DBC signal definitions
- **Value tables**: Support for enumerated values and their text descriptions
- **Multiplexed signals**: Handle complex multiplexed CAN signals
- **Optimized output**: Efficient binary streaming to Python for MF4 generation
- **Unit preservation**: Maintain signal units from DBC files in the output
- **Timestamp handling**: Ensure monotonic timestamps in output data

## Prerequisites

- **Rust** (latest stable version)
- **Python 3.7+** with the following packages:
  - `asammdf`
  - `numpy`
  - `tqdm`

## Installation

1. Clone the repository:
```bash
git clone https://github.com/JanSpindler/blf2mdf.git
cd blf2mdf
```

2. Build and install the Rust application:
```bash
cargo install --path .
```

## Usage

1. **Run the application**:
```bash
blf2mdf
```

2. **Follow the interactive prompts**:
   - Enter the number of CAN buses in your setup
   - Select the BLF files you want to convert using the file dialog
   - For each bus, select the corresponding DBC files using the file dialog

3. **Output**: The tool will generate MF4 files with the same base name as your BLF files in the same directory.

### Example Workflow

```
Enter the number of CAN busses: 2
[File dialog opens] -> Select: data1.blf, data2.blf
[File dialog opens] -> Select DBC files for bus 1: engine.dbc, transmission.dbc
[File dialog opens] -> Select DBC files for bus 2: body.dbc, chassis.dbc
```

This will process:
- `data1.blf` → `data1.mf4`
- `data2.blf` → `data2.mf4`
