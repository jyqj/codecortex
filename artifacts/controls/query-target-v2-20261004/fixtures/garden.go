package garden

// Lantern holds the lighting state.
type Lantern struct {
    Ready bool
    State bool
}

// Ignite changes the lighting state.
func (l *Lantern) Ignite() {
    l.Ready = true
}

// Listener accepts a signal.
type Listener interface {
    Listen(signal int) error
}

// SignalLabel names a signal.
type SignalLabel string

// Ignite is also a type, unrelated to the method.
type Ignite struct { Count int }
