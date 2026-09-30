from decoder import decode_frame


def process_packet(raw):
    tag, payload = decode_frame(raw)
    return {"kind": tag, "body": payload}
