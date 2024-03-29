use chrono::NaiveDate;
use crate::fiat;
use crate::RequestError;
use beancount_core::{Amount, Date, Price};
use beancount_render::{BasicRenderer, Renderer};
use chrono::serde::ts_milliseconds;
use chrono::{DateTime, Utc};
use rust_decimal::{prelude::*, Decimal};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fs::File;
use std::io::BufWriter;
use std::iter::successors;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub ticker: String,
    pub id: String,
    path: PathBuf,
}

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PricePoint {
    price_usd: String,
    #[serde(with = "ts_milliseconds")]
    time: DateTime<Utc>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct History {
    data: Option<Vec<PricePoint>>,
}

/// Fetch the price history from coincap.
pub(crate) async fn fetch_price_history(id: &str, start: NaiveDate) -> Result<History, RequestError> {
    let start = start.and_hms(0, 0, 0).timestamp_millis();
    let end = Utc::now().timestamp_millis();
    reqwest::get(&format!(
        "https://api.coincap.io/v2/assets/{}/history?interval=d1&start={}&end={}",
        id, start, end
    ))
    .await
    .map_err(RequestError::PriceHistory)?
    .json::<History>()
    .await
    .map_err(RequestError::PriceHistory)
}

pub(crate) async fn generate_file(
    config: &Config,
    renderer: BasicRenderer,
    base_currency: &str,
    start_date: NaiveDate
) -> Result<(), RequestError> {
    use RequestError::{
        BeancountFileCreationFailed, InvalidPrice, ParsePriceError, PriceDataError,
    };
    if let Some(asset_prices) = fetch_price_history(&config.id, start_date).await?.data {
        // Get the available date range for this asset.
        let first = asset_prices.first().unwrap().time.date().naive_utc();
        let last = asset_prices.last().unwrap().time.date().naive_utc();

        // Get the exchange rate for this date range.
        let usd_eur = fiat::fetch_exchange_rate_history(first, last, "USD", base_currency).await?;

        // Create the destination file
        let file = File::create(&config.path).map_err(BeancountFileCreationFailed)?;
        let mut buf_writer = BufWriter::new(file);

        // Write the prices
        for line in asset_prices {
            let date = line.time.date().naive_utc();

            // Find a previous day with an exchange rate
            if let Some(usd_eur) = successors(Some(date), chrono::NaiveDate::pred_opt)
                .find_map(|date| usd_eur.rate_at(date))
            {
                let amount = Amount::builder()
                    .currency(Cow::from(base_currency))
                    .num(
                        Decimal::from_f64(
                            line.price_usd.parse::<f64>().map_err(ParsePriceError)? / usd_eur,
                        )
                        .ok_or(InvalidPrice)?,
                    )
                    .build();
                let price = Price::builder()
                    .date(Date::from(date))
                    .currency(Cow::from(&config.ticker))
                    .amount(amount)
                    .build();
                renderer
                    .render(&price, &mut buf_writer)
                    .map_err(PriceDataError)?;
            }
        }
    }
    Ok(())
}
