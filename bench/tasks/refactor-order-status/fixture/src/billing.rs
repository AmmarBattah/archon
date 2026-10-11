use crate::order::Order;

pub fn pay(order: &mut Order) -> Result<(), String> {
    if order.status != "pending" {
        return Err(format!("order {} cannot be paid: it is {}", order.id, order.status));
    }
    order.status = "paid".to_string();
    Ok(())
}

/// Money still owed: the totals of every unpaid, uncancelled order.
pub fn outstanding_cents(orders: &[Order]) -> u64 {
    orders
        .iter()
        .filter(|o| o.status == "pending")
        .map(|o| o.total_cents)
        .sum()
}
