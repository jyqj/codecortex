package beacon

// Beacon stores the garden state.
type Beacon struct {
    Ready bool
}

// Commit changes the ready state.
func (c *Beacon) Commit() {
    c.Ready = true
}

// Sink accepts a seed.
type Sink interface {
    Accept(seed int) error
}

// SeedCode names a seed.
type SeedCode string
