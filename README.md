# CAN Bus Log Processor

This application processes CAN bus log files (`.blf`) and associates them with corresponding DBC files to decode the data. It allows users to select log files and DBC files interactively and processes the data for analysis.

## Features

- Interactive file selection for `.blf` and `.dbc` files.
- Supports multiple CAN busses.
- Decodes CAN bus log data using DBC files.
- Outputs processed data for further analysis.

## Prerequisites

- Rust (latest stable version)
- A terminal or command-line interface
- `.blf` log files and `.dbc` files for decoding

## Installation

'''sh
cargo install --path .
'''