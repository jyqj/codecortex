pub struct Vessel { pub ready: bool }
pub trait Sensor { fn ping(&self); }
pub type Tag = String;
impl Vessel { pub fn pulse(&self) -> bool { self.ready } }
pub fn pulse() -> bool { true }
pub fn rs() -> bool { false }
