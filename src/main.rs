use axum::{Json, Router, serve};
use axum::extract::{Query, State};
use axum::routing::get;
use chrono::{DateTime, Local};
use log::info;
use reqwest::{Client, StatusCode};
use serde_derive::{Deserialize, Serialize};
use tokio::net::TcpListener;

const APP_NAME: &str = "electricity-price-optimiser";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting {}", APP_NAME);

    let api_key = std::env::var("TIBBER_API_KEY").expect("TIBBER_API_KEY must be set");
    let port = std::env::var("PORT").unwrap_or("3000".to_string());

    let app_state = AppState::new(api_key);


    let router = Router::new()
        .route("/time-slots", get(get_time_slots))
        .with_state(app_state.clone())
        ;

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();

    info!("Now listening on port {}", port);

    serve(listener, router).await.unwrap();

    info!("Shutting down {}", APP_NAME);
}

#[derive(Clone)]
struct AppState {
    api_key: String,
}

impl AppState {
    fn new(api_key: String) -> Self {
        Self { api_key }
    }
}


#[derive(Debug, Clone, Deserialize)]
struct Windows {
    durations: String,
}

impl Default for Windows {
    fn default() -> Self {
        Self {
            durations: "".to_string()
        }
    }
}

async fn get_time_slots(State(state): State<AppState>, windows: Option<Query<Windows>>) -> (StatusCode, Json<Vec<OptimalWindow>>) {
    let prices = get_prices(&state.api_key).await.unwrap();

    let durations = windows
        .unwrap_or_default()
        .0
        .durations
        .split(",")
        .map(|s| s.parse::<usize>().ok())
        .filter(|o| o.is_some())
        .map(|o| o.unwrap())
        .collect::<Vec<usize>>();

    let optimal_windows = calculate_optimal_windows(prices, durations);

    (StatusCode::OK, Json(optimal_windows))
}


#[derive(Debug, Clone, Serialize)]
struct OptimalWindow {
    duration: usize,
    from: DateTime<Local>,
    to: DateTime<Local>,
    average_price: f64,
    prices: Vec<PricePoint>,
}

fn calculate_optimal_windows(prices: Vec<PricePoint>, durations: Vec<usize>) -> Vec<OptimalWindow> {
    let mut optimal_windows: Vec<OptimalWindow> = vec![];

    for duration in durations {
        let mut possible_windows: Vec<OptimalWindow> = vec![];

        if prices.len() < duration {
            continue;
        }

        info!("Calculating optimal window for a {} hour duration", duration);

        for i in 0..prices.len() {
            if i + duration >= prices.len() {
                break;
            }

            let starting_price_point = &prices[i];
            let mut total_price_for_window = 0.0;

            for j in 0..duration {
                if i + j >= prices.len() - 1 {
                    break;
                }

                total_price_for_window += prices[i + j].total;
            }


            possible_windows.push(OptimalWindow {
                duration,
                from: DateTime::parse_from_rfc3339(&starting_price_point.starts_at).unwrap().with_timezone(&Local),
                to: DateTime::parse_from_rfc3339(&prices[i + duration - 1].starts_at).unwrap().with_timezone(&Local),
                average_price: total_price_for_window / duration as f64,
                prices: prices[i..i + duration].to_vec(),
            });
        }

        let min = possible_windows.iter().min_by(|a, b| a.average_price.partial_cmp(&b.average_price).unwrap()).unwrap();
        optimal_windows.push(min.clone());
    }

    info!("Calculated {} optimal windows", optimal_windows.len());

    return optimal_windows;
}

async fn get_prices(api_key: &str) -> reqwest::Result<Vec<PricePoint>> {
    info!("Fetching prices from tibber");

    let query = r#"{ "query": "{ viewer { homes { currentSubscription { priceInfo { today { total startsAt } }}}}}" }"#;

    let client = Client::new();

    let response = client
        .post("https://api.tibber.com/v1-beta/gql")
        .header("Authorization", api_key)
        .header("Content-Type", "application/json")
        .body(query)
        .send()
        .await?;

    let body = response.text().await?;

    let data = serde_json::from_str::<Response>(&body).expect("Failed to parse tibber's response");

    info!("Fetched {} prices from tibber", data.data.viewer.homes[0].current_subscription.price_info.today.len());

    return Ok(data.data.viewer.homes[0].current_subscription.price_info.today.clone());
}

#[derive(Deserialize, Debug)]
struct Response {
    data: Data,
}

#[derive(Deserialize, Debug)]
struct Data {
    viewer: Viewer,
}

#[derive(Deserialize, Debug)]
struct Viewer {
    homes: Vec<Home>,
}

#[derive(Deserialize, Debug)]
struct Home {
    #[serde(rename = "currentSubscription")]
    current_subscription: CurrentSubscription,
}

#[derive(Deserialize, Debug)]
struct CurrentSubscription {
    #[serde(rename = "priceInfo")]
    price_info: PriceInfo,
}

#[derive(Deserialize, Debug)]
struct PriceInfo {
    today: Vec<PricePoint>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PricePoint {
    total: f64,
    #[serde(rename = "startsAt")]
    starts_at: String,
}