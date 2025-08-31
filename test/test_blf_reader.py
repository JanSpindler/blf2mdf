import can
import tqdm


if __name__ == '__main__':
    with can.BLFReader("./data/Measurement_32.blf") as log:
        for msg in tqdm.tqdm(log):
            pass
