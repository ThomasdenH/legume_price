use crate::RequestError;
use beancount_core::{Amount, Date, Price};
use beancount_render::{BasicRenderer, Renderer};
use chrono::Local;
use chrono::NaiveDate;
use rust_decimal::{prelude::*, Decimal};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub symbol: String,
    path: PathBuf,
}

pub(crate) async fn generate_file(
    config: &Config,
    renderer: BasicRenderer,
    first: NaiveDate,
    base_currency: &str,
) -> Result<(), RequestError> {
    use RequestError::{BeancountFileCreationFailed, InvalidPrice, PriceDataError};
    let last = Local::now().date().naive_local();
    let rates = fetch_exchange_rate_history(first, last, &config.symbol, base_currency).await?;

    // Create the destination file
    let file = File::create(&config.path).map_err(BeancountFileCreationFailed)?;
    let mut buf_writer = BufWriter::new(file);

    let mut date_rates: Vec<_> = rates.all();

    date_rates.sort_by_key(|(date, _rate)| *date);

    for (date, rate) in date_rates {
        let amount = Amount::builder()
            .currency(Cow::from(base_currency))
            .num(Decimal::from_f64(1.0 / rate).ok_or(InvalidPrice)?)
            .build();
        let price = Price::builder()
            .date(Date::from(date))
            .currency(Cow::from(&config.symbol))
            .amount(amount)
            .build();
        renderer
            .render(&price, &mut buf_writer)
            .map_err(PriceDataError)?;
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum Rates<'a> {
    /// The rates are equal
    Equal,
    /// The rates in the, for a date and a currency.
    Conversion {
        symbol: &'a str,
        rate: ConversionRate,
    },
}

impl<'a> Rates<'a> {
    pub(crate) fn rate_at(&self, date: NaiveDate) -> Option<f64> {
        match self {
            Rates::Equal => Some(1.0),
            Rates::Conversion { symbol, rate } => {
                rate.rates.get(&date).and_then(|r| r.get(*symbol).copied())
            }
        }
    }

    pub(crate) fn all(&self) -> Vec<(NaiveDate, f64)> {
        match self {
            Rates::Equal => Vec::new(),
            Rates::Conversion {
                symbol,
                rate: ConversionRate { rates },
                ..
            } => rates
                .iter()
                .filter_map(|(date, rates)| rates.get(*symbol).map(|rate| (*date, *rate)))
                .collect(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub(crate) struct ConversionRate {
    rates: HashMap<NaiveDate, HashMap<String, f64>>,
}

/// Fetch the `symbol' EUR exchange rate, expressed in euros.
pub(crate) async fn fetch_exchange_rate_history<'a>(
    first: NaiveDate,
    last: NaiveDate,
    symbol: &'a str,
    base_currency: &str,
) -> Result<Rates<'a>, RequestError> {
    if symbol == base_currency {
        Ok(Rates::Equal)
    } else {
        reqwest::get(&format!(
            "https://api.exchangeratesapi.io/history?start_at={}&end_at={}&symbols={}&base={}",
            first, last, symbol, base_currency
        ))
        .await
        .map_err(RequestError::ExchangeRate)?
        .json::<ConversionRate>()
        .await
        .map_err(RequestError::ExchangeRate)
        .map(|rate| Rates::Conversion { symbol, rate })
    }
}
