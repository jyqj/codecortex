from decoder import decode_frame


def safe_decode(raw):
    try:
        return decode_frame(raw)
    except ValueError:
        return None
