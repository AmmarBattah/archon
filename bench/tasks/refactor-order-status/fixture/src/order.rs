#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub id: u32,
    pub customer: String,
    pub total_cents: u64,
    pub status: String,
}

impl Order {
    pub fn new(id: u32, customer: &str, total_cents: u64) -> Self {
        Order {
            id,
            customer: customer.to_string(),
            total_cents,
            status: "pending".to_string(),
        }
    }

    /// Still expected to move: not shipped and not cancelled.
    pub fn is_open(&self) -> bool {
        self.status == "pending" || self.status == "paid"
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        if self.status == "shipped" || self.status == "cancelled" {
            return Err(format!("order {} cannot be cancelled: it is {}", self.id, self.status));
        }
        self.status = "cancelled".to_string();
        Ok(())
    }
}
