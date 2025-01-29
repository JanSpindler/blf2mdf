import cantools
import can
import tqdm
from asammdf import Signal, MDF


signal_data_dict = {}


def gen_signal_name(bus_name, message_name, signal_name):
    return f'{bus_name}::{message_name}::{signal_name}'


def initialize_signal_dict(db):
    for message in tqdm.tqdm(db.messages):
        for signal in message.signals:
            signal_data_dict[gen_signal_name('CAN1', message.name, signal.name)] = []


def read_can_signals(log, db):
    # Init
    start_time = None

    # Iterate over all messages
    for msg in tqdm.tqdm(log):
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


if __name__ == '__main__':
    # Load DBC file
    db = cantools.database.load_file('data/DBC/GXe_CAN1.dbc')
    initialize_signal_dict(db)

    # Read CAN signals
    with can.BLFReader('data/Measurement_40.blf') as log:
        read_can_signals(log, db)

    # Create MDF file
    mdf = MDF(version='4.11')
    for signal_name, signal_data in tqdm.tqdm(signal_data_dict.items()):
        if not signal_data:
            continue
        timestamps, values = zip(*signal_data)
        signal = Signal(samples=values, timestamps=timestamps, name=signal_name, encoding='utf-8')
        mdf.append(signal)
    mdf.save('result.mdf', overwrite=True, compression=2)
