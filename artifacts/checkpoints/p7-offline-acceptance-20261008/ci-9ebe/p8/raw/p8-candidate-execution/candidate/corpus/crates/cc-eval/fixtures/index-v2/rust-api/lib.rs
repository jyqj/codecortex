mod provider;
use provider::transform;

pub fn entry(value: i32) -> i32 {
    transform(value)
}
