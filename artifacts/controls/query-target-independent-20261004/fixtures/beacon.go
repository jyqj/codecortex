package lights
type BeaconGo struct { Ready bool }
type Observer interface { pulse() }
type TagGo = string
func (b *BeaconGo) pulse() int { return 5 }
func plain() int { return 6 }
