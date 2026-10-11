use std::collections::BTreeMap;

use crate::order::Order;

const KNOWN: [&str; 4] = ["pending", "paid", "shipped", "cancelled"];

/// How many orders are in each status, sorted by status name.
pub fn status_counts(orders: &[Order]) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for order in orders {
        *counts.entry(order.status.clone()).or_default() += 1;
    }
    counts.into_iter().collect()
}

/// One CSV line: `id,customer,total_cents,status`.
pub fn to_line(order: &Order) -> String {
    format!("{},{},{},{}", order.id, order.customer, order.total_cents, order.status)
}

pub fn from_line(line: &str) -> Result<Order, String> {
    let fields: Vec<&str> = line.trim().split(',').collect();
    if fields.len() != 4 {
        return Err(format!("expected 4 fields, got {}", fields.len()));
    }
    let id = fields[0].parse().map_err(|e| format!("bad id {:?}: {e}", fields[0]))?;
    let total_cents = fields[2].parse().map_err(|e| format!("bad total {:?}: {e}", fields[2]))?;
    if !KNOWN.contains(&fields[3]) {
        return Err(format!("unknown status {:?}", fields[3]));
    }
    Ok(Order {
        id,
        customer: fields[1].to_string(),
        total_cents,
        status: fields[3].to_string(),
    })
}
