use crate::order::Order;

pub fn ship(order: &mut Order) -> Result<(), String> {
    if order.status != "paid" {
        return Err(format!("order {} cannot be shipped: it is {}", order.id, order.status));
    }
    order.status = "shipped".to_string();
    Ok(())
}

/// Ids of paid orders waiting for shipment, in input order.
pub fn ready_to_ship(orders: &[Order]) -> Vec<u32> {
    orders
        .iter()
        .filter(|o| o.status == "paid")
        .map(|o| o.id)
        .collect()
}
