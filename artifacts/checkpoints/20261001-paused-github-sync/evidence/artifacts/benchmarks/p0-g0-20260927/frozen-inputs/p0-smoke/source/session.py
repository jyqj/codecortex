"""Session renewal and bounded retry behavior."""

def renew_session(token, issued_at, now, lifetime=3600):
    """Replace an expired session token while preserving a still-valid token."""
    if now - issued_at >= lifetime:
        return {"token": token + ":renewed", "issued_at": now}
    return {"token": token, "issued_at": issued_at}


def retry_connection(connect, attempts=3):
    """Stop reconnecting after the configured attempt budget is consumed."""
    for _ in range(attempts):
        try:
            return connect()
        except ConnectionError:
            continue
    raise TimeoutError("connection retry budget exhausted")
