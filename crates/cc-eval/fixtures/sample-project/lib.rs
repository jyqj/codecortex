pub fn calculate_total(items: &[f64]) -> f64 {
    items.iter().sum()
}

pub fn format_price(amount: f64) -> String {
    format!("${:.2}", amount)
}

pub fn process_order(items: &[f64]) -> String {
    let total = calculate_total(items);
    format_price(total)
}

fn unused_internal() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_total() {
        assert_eq!(calculate_total(&[1.0, 2.0, 3.0]), 6.0);
    }
}
