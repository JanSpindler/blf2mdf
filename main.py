import cantools
import can
import cantools.database
from tqdm import tqdm
from asammdf import Signal, MDF
from PyQt6.QtWidgets import QApplication, QMainWindow, QFileDialog, QLabel, QPushButton
from PyQt6.QtCore import Qt
import sys


signal_data_dict = {}
signal_choices_dict = {}


def gen_signal_name(bus_name, message_name, signal_name):
    return f'{bus_name}::{message_name}::{signal_name}'


def initialize_signal_dict(can1_dbs, can2_dbs, can3_dbs):
    # Clear
    signal_data_dict.clear()
    signal_choices_dict.clear()

    # CAN 1
    for can1_db in can1_dbs:
        for message in can1_db.messages:
            for signal in message.signals:
                signal_data_dict[gen_signal_name('CAN1', message.name, signal.name)] = []

    # CAN 2
    for can2_db in can2_dbs:
        for message in can2_db.messages:
            for signal in message.signals:
                signal_data_dict[gen_signal_name('CAN2', message.name, signal.name)] = []

    # CAN 3
    for can3_db in can3_dbs:
        for message in can3_db.messages:
            for signal in message.signals:
                signal_data_dict[gen_signal_name('CAN3', message.name, signal.name)] = []


def read_can_signals(log, can1_dbs, can2_dbs, can3_dbs):
    # Init
    start_time = None

    # Iterate over all messages
    can_bus_list = [can1_dbs, can2_dbs, can3_dbs]
    can_str_list = ['CAN1', 'CAN2', 'CAN3']
    for msg in tqdm(log):
        # Get start time
        if start_time is None:
            start_time = msg.timestamp

        # Get correct CAN bus list
        dbs = can_bus_list[msg.channel]

        # Iterate over db in dbs
        for db in dbs:
            # Try to find message by id
            try:
                message = db.get_message_by_frame_id(msg.arbitration_id)
            except KeyError:
                continue

            # Get relative timestamp
            timestamp = msg.timestamp - start_time

            # Try to decode message
            try:
                decoded_signals = db.decode_message(msg.arbitration_id, msg.data)
            except cantools.database.DecodeError:
                continue

            # Iterate over all signals in message
            for signal_name, signal_value in decoded_signals.items():
                full_signal_name = gen_signal_name(can_str_list[msg.channel], message.name, signal_name)
                signal_data_dict[full_signal_name].append((timestamp, signal_value))
                choices = message.get_signal_by_name(signal_name).choices
                if choices is not None:
                    choices = {value: name.name for value, name in choices.items()}
                    signal_choices_dict[full_signal_name] = choices

            # Do not check other db files. Assume no msg id collision
            break


def asciify(string):
    replacements = {
        'ä': 'ae',
        'ö': 'oe',
        'ü': 'ue',
        'Ä': 'Ae',
        'Ö': 'Oe',
        'Ü': 'Ue',
        'ß': 'ss'
    }
    string = ''.join(replacements.get(c, c) for c in string)
    string = string.encode('ascii', 'ignore').decode('ascii')
    for german_char, replacement in replacements.items():
        string = string.replace(german_char, replacement)    
    return string.encode('ascii')


def process_blf_files(blf_file_paths, can1_db_paths, can2_db_paths, can3_db_paths):
    # Load CAN DBC files
    can1_dbs = [cantools.database.load_file(can1_db_path) for can1_db_path in can1_db_paths]
    can2_dbs = [cantools.database.load_file(can2_db_path) for can2_db_path in can2_db_paths]
    can3_dbs = [cantools.database.load_file(can3_db_path) for can3_db_path in can3_db_paths]

    # Iterate over all BLF files
    for blf_file_path in blf_file_paths:
        # Init signal data dict
        initialize_signal_dict(can1_dbs, can2_dbs, can3_dbs)
        print(f'Processing {blf_file_path}')

        # Read CAN signals
        with can.BLFReader(blf_file_path) as log:
            read_can_signals(log, can1_dbs, can2_dbs, can3_dbs)

        # Create MDF file
        mdf = MDF(version='4.20')
        for signal_name, signal_data in tqdm(signal_data_dict.items()):
            # Check if data is empty
            if not signal_data:
                continue

            # Seperate timestamps and values
            timestamps, values = zip(*signal_data)

            # Handle named value signals
            conversion = None
            if isinstance(values[0], cantools.database.namedsignalvalue.NamedSignalValue):
                # Conversion
                initial_dict = signal_choices_dict[signal_name]
                conversion = {}
                for idx, (value, name) in enumerate(initial_dict.items()):
                    conversion[f'val_{idx}'] = value
                    conversion[f'text_{idx}'] = asciify(name)

                # Samples are raw
                values = [value.value for value in values]

            # Double check for NamedSignalValue
            values = [value.value if isinstance(value, cantools.database.namedsignalvalue.NamedSignalValue) 
                      else value 
                      for value in values]

            # Add signal
            signal = Signal(
                samples=values, 
                timestamps=timestamps, 
                name=signal_name, 
                encoding='utf-8',
                conversion=conversion)
            mdf.append(signal)

        # Save MDF file
        result_file_name = blf_file_path.replace('.blf', '.mf4')
        mdf.save(result_file_name, overwrite=True, compression=2)
        print(f'Saved {result_file_name}')


class MainWindow(QMainWindow):
    def __init__(self):
        # Window
        super().__init__()
        self.setWindowTitle("blf2mdf")
        self.setGeometry(100, 100, 800, 700)
        self.setFixedSize(800, 700)

        # BLF file selection
        self.select_blf_button = QPushButton("Select .blf files", self)
        self.select_blf_button.clicked.connect(self.select_blf_file)
        self.select_blf_button.setGeometry(10, 10, 150, 30)

        self.selected_blf_label = QLabel("Selected .blf files:", self)
        self.selected_blf_label.setGeometry(10, 50, 390, 500)
        self.selected_blf_label.setWordWrap(True)
        self.selected_blf_label.setStyleSheet("background-color: #505050;")
        self.selected_blf_label.setAlignment(Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignLeft)
        self.selected_blf_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)

        # CAN 1 DBC file selection
        self.select_can1_dbc_button = QPushButton("Select CAN1 .dbc files", self)
        self.select_can1_dbc_button.setGeometry(410, 10, 150, 30)
        self.select_can1_dbc_button.clicked.connect(self.select_can1_dbc_file)

        self.selected_can1_dbc_label = QLabel("Selected files:", self)
        self.selected_can1_dbc_label.setGeometry(410, 50, 380, 100)
        self.selected_can1_dbc_label.setWordWrap(True)
        self.selected_can1_dbc_label.setStyleSheet("background-color: #505050;")
        self.selected_can1_dbc_label.setAlignment(Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignLeft)
        self.selected_can1_dbc_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)

        # CAN 2 DBC file selection
        self.select_can2_dbc_button = QPushButton("Select CAN2 .dbc files", self)
        self.select_can2_dbc_button.setGeometry(410, 210, 150, 30)
        self.select_can2_dbc_button.clicked.connect(self.select_can2_dbc_file)

        self.selected_can2_dbc_label = QLabel("Selected files:", self)
        self.selected_can2_dbc_label.setGeometry(410, 250, 380, 100)
        self.selected_can2_dbc_label.setWordWrap(True)
        self.selected_can2_dbc_label.setStyleSheet("background-color: #505050;")
        self.selected_can2_dbc_label.setAlignment(Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignLeft)
        self.selected_can2_dbc_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)

        # CAN 3 DBC file selection
        self.select_can3_dbc_button = QPushButton("Select CAN3 .dbc files", self)
        self.select_can3_dbc_button.setGeometry(410, 410, 150, 30)
        self.select_can3_dbc_button.clicked.connect(self.select_can3_dbc_file)

        self.selected_can3_dbc_label = QLabel("Selected files:", self)
        self.selected_can3_dbc_label.setGeometry(410, 450, 380, 100)
        self.selected_can3_dbc_label.setWordWrap(True)
        self.selected_can3_dbc_label.setStyleSheet("background-color: #505050;")
        self.selected_can3_dbc_label.setAlignment(Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignLeft)
        self.selected_can3_dbc_label.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)

        # Process button
        self.process_button = QPushButton("Process", self)
        self.process_button.setGeometry(10, 610, 150, 30)
        self.process_button.clicked.connect(self.process_blfs)


    def select_blf_file(self):
        file_dialog = QFileDialog(self)
        file_dialog.setFileMode(QFileDialog.FileMode.ExistingFiles)
        file_dialog.setNameFilter("BLF files (*.blf)")
        if file_dialog.exec():
            self.selected_blf = file_dialog.selectedFiles()
            self.selected_blf_label.setText("Selected .blf files:\n" + "\n".join(self.selected_blf))


    def select_can1_dbc_file(self):
        file_dialog = QFileDialog(self)
        file_dialog.setNameFilter("DBC files (*.dbc)")
        file_dialog.setFileMode(QFileDialog.FileMode.ExistingFiles)
        if file_dialog.exec():
            self.selected_can1_dbc = file_dialog.selectedFiles()
            self.selected_can1_dbc_label.setText("Selected files:\n" + "\n".join(self.selected_can1_dbc))

    
    def select_can2_dbc_file(self):
        file_dialog = QFileDialog(self)
        file_dialog.setNameFilter("DBC files (*.dbc)")
        file_dialog.setFileMode(QFileDialog.FileMode.ExistingFiles)
        if file_dialog.exec():
            self.selected_can2_dbc = file_dialog.selectedFiles()
            self.selected_can2_dbc_label.setText("Selected files:\n" + "\n".join(self.selected_can2_dbc))

    
    def select_can3_dbc_file(self):
        file_dialog = QFileDialog(self)
        file_dialog.setNameFilter("DBC files (*.dbc)")
        file_dialog.setFileMode(QFileDialog.FileMode.ExistingFiles)
        if file_dialog.exec():
            self.selected_can3_dbc = file_dialog.selectedFiles()
            self.selected_can3_dbc_label.setText("Selected files:\n" + "\n".join(self.selected_can3_dbc))


    def process_blfs(self):
        if not hasattr(self, 'selected_blf') or not hasattr(self, 'selected_can1_dbc') or not hasattr(self, 'selected_can2_dbc') or not hasattr(self, 'selected_can3_dbc'):
            return
        process_blf_files(
            self.selected_blf, 
            self.selected_can1_dbc, 
            self.selected_can2_dbc, 
            self.selected_can3_dbc)


if __name__ == '__main__':
    # Select file
    app = QApplication(sys.argv)
    window = MainWindow()
    window.show()
    sys.exit(app.exec())
