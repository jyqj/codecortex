def decode_frame(raw):
    if not raw:
        raise ValueError("empty frame")
    tag, payload = raw.split(b":", 1)
    return tag.decode("ascii"), payload
