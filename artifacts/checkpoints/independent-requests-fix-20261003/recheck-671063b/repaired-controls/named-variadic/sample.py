class Packet: pass
class Fault(Exception):
    def __init__(self, packets: tuple[Packet, ...] | None = None):
        pass
