class Packet: pass
class Fault(Exception):
    def __init__(self, retry_history: tuple[int, ...] | None = None):
        pass
