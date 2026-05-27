use axum::{
    extract::Path,
    routing::{get, post, delete},
    Json, Router,
};
use serde::{Deserialize, Serialize};

// Re-use calculate_total from the sibling module.
use crate::lib::calculate_total;

/// Request body for creating an order.
#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    pub customer_name: String,
    pub items: Vec<f64>,
}

/// Response body for order endpoints.
#[derive(Debug, Serialize)]
pub struct OrderResponse {
    pub order_id: u64,
    pub customer_name: String,
    pub total: f64,
    pub formatted_total: String,
}

/// Response body for a simple status check.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

/// Build the Axum router with all order-related routes.
pub fn order_routes() -> Router {
    Router::new()
        .route("/orders", get(list_orders))
        .route("/orders", post(create_order))
        .route("/orders/:id", get(get_order))
        .route("/orders/:id", delete(delete_order))
}

/// GET /orders — list all orders.
async fn list_orders() -> Json<Vec<OrderResponse>> {
    let sample = OrderResponse {
        order_id: 1,
        customer_name: "Alice".to_string(),
        total: 42.0,
        formatted_total: "$42.00".to_string(),
    };
    Json(vec![sample])
}

/// POST /orders — create a new order, computing the total via calculate_total.
async fn create_order(Json(payload): Json<CreateOrderRequest>) -> Json<OrderResponse> {
    let total = calculate_total(&payload.items);
    let formatted = format!("${:.2}", total);
    Json(OrderResponse {
        order_id: 42,
        customer_name: payload.customer_name,
        total,
        formatted_total: formatted,
    })
}

/// GET /orders/:id — retrieve a single order by ID.
async fn get_order(Path(order_id): Path<u64>) -> Json<OrderResponse> {
    Json(OrderResponse {
        order_id,
        customer_name: "Bob".to_string(),
        total: 99.99,
        formatted_total: "$99.99".to_string(),
    })
}

/// DELETE /orders/:id — remove an order by ID.
async fn delete_order(Path(order_id): Path<u64>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "deleted": true,
        "order_id": order_id
    }))
}

/// Health endpoint, not wired through order_routes.
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: "0.1.0".to_string(),
    })
}
