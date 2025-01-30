import cantools
import can
from tqdm import tqdm
from asammdf import Signal, MDF
from PyQt6.QtWidgets import QApplication, QMainWindow, QFileDialog, QLabel, QPushButton
from PyQt6.QtCore import Qt
import sys


signal_data_dict = {}


def gen_signal_name(bus_name, message_name, signal_name):
    return f'{bus_name}::{message_name}::{signal_name}'


def initialize_signal_dict(db):
    for message in tqdm(db.messages):
        for signal in message.signals:
            signal_data_dict[gen_signal_name('CAN1', message.name, signal.name)] = []


def read_can_signals(log, db):
    # Init
    start_time = None

    # Iterate over all messages
    for msg in tqdm(log):
        # Get start time
        if start_time is None:
            start_time = msg.timestamp

        # Try to find message by id
        try:
            message = db.get_message_by_frame_id(msg.arbitration_id)
        except KeyError:
            continue

        #
        timestamp = msg.timestamp - start_time
        decoded_signals = db.decode_message(msg.arbitration_id, msg.data)

        # Iterate over all signals in message
        for signal_name, signal_value in decoded_signals.items():
            signal_data_dict[gen_signal_name('CAN1', message.name, signal_name)].append((timestamp, signal_value))


class MainWindow(QMainWindow):
    def __init__(self):
        # Window
        super().__init__()
        self.setWindowTitle("blf2mdf")
        self.setGeometry(100, 100, 800, 560)
        self.setFixedSize(800, 560)

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


if __name__ == '__main__':
    # Select file
    app = QApplication(sys.argv)
    window = MainWindow()
    window.show()
    sys.exit(app.exec())

    # Load DBC file
    db = cantools.database.load_file('data/DBC/GXe_CAN1.dbc')
    initialize_signal_dict(db)

    # Read CAN signals
    with can.BLFReader('data/Measurement_40.blf') as log:
        read_can_signals(log, db)

    # Create MDF file
    mdf = MDF(version='4.11')
    for signal_name, signal_data in tqdm(signal_data_dict.items()):
        if not signal_data:
            continue
        timestamps, values = zip(*signal_data)
        signal = Signal(samples=values, timestamps=timestamps, name=signal_name, encoding='utf-8')
        mdf.append(signal)
    mdf.save('result.mdf', overwrite=True, compression=2)
