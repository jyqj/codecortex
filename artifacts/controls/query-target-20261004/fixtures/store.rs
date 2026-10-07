pub struct Store { pub ready: bool }
impl Store {
    pub fn refresh(&mut self) { self.ready = true; }
}
pub trait Observer { fn observe(&self); }
pub type Label = String;
