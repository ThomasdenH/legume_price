#![forbid(unsafe_code)]
#![deny(bare_trait_objects)]
#![deny(elided_lifetimes_in_paths)]
#![deny(missing_debug_implementations)]

use beancount_render::BasicRenderer;
use chrono::NaiveDate;
use serde::Deserialize;
use std::path::PathBuf;
use structopt::StructOpt;
use thiserror::*;

mod coincap;
mod fiat;

/// The error type for the application main.
#[derive(Debug, Error)]
enum Error {
    #[error("could not open config file")]
    ConfigFile(#[source] std::io::Error),
    #[error("could not parse yaml")]
    ConfigFileYaml(#[source] serde_yaml::Error),
    #[error("could not update token {id}")]
    Request {
        id: String,
        #[source]
        source: RequestError,
    },
}

#[derive(Debug, StructOpt)]
#[structopt(name = "beancount-cryptocurrency-export")]
struct Opt {
    #[structopt(short = "c", long = "config", parse(from_os_str))]
    config: PathBuf,
}

#[derive(Deserialize)]
struct Config {
    start: NaiveDate,
    base_currency: String,
    currencies: Vec<CurrencyConfig>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum CurrencyConfig {
    Coincap(coincap::Config),
    Fiat(fiat::Config),
}

#[derive(Debug, Error)]
enum RequestError {
    #[error("price history request failed")]
    PriceHistory(#[source] reqwest::Error),
    #[error("could not fetch exchange rate")]
    ExchangeRate(#[source] reqwest::Error),
    #[error("could not create the beancount file")]
    BeancountFileCreationFailed(#[source] std::io::Error),
    #[error("could not create beancount file")]
    ParsePriceError(#[source] std::num::ParseFloatError),
    #[error("invalid price")]
    InvalidPrice,
    #[error("could not render price data")]
    PriceDataError(#[source] beancount_render::BasicRendererError),
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::from_args();
    let yaml_file = std::fs::File::open(&opt.config).map_err(Error::ConfigFile)?;
    let root_config: Config = serde_yaml::from_reader(yaml_file).map_err(Error::ConfigFileYaml)?;
    let renderer = BasicRenderer::default();
    for currency in root_config.currencies {
        match currency {
            CurrencyConfig::Coincap(currency) => {
                coincap::generate_file(&currency, &renderer, &root_config.base_currency)
                    .await
                    .map_err(|source| Error::Request {
                        id: currency.id.to_string(),
                        source,
                    })?
            }
            CurrencyConfig::Fiat(fiat) => fiat::generate_file(
                &fiat,
                &renderer,
                root_config.start,
                &root_config.base_currency,
            )
            .await
            .map_err(|source| Error::Request {
                id: fiat.symbol.to_string(),
                source,
            })?,
        }
    }
    Ok(())
}
