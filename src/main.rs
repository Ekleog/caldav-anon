use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::{ensure, Context};
use rocket::{http::Status, response::status};
use structopt::StructOpt;
use tracing::warn;

/// Anonymize the contents of iCal URLs while keeping the time slots
#[derive(Debug, StructOpt)]
struct Opt {
    /// Path to the configuration file.
    ///
    /// The configuration file only contains a `[calendars]` section, where each element is
    /// formatted as `path = "remote_url"`. Then, `http://localhost:<port>/<path>` will return an
    /// anonymized version of `remote_url`.
    #[structopt(short, long)]
    config_file: std::path::PathBuf,

    /// Address on which to listen
    #[structopt(short, long, default_value = "127.0.0.1")]
    address: IpAddr,

    /// Port on which to listen
    #[structopt(short, long, default_value = "8000")]
    port: u16,
}

#[derive(serde::Deserialize)]
struct Config {
    calendars: HashMap<String, url::Url>,
}

async fn parse_remote_ics(url: &url::Url) -> anyhow::Result<icalendar::Calendar> {
    // Fetch the remote ICS file
    let response = reqwest::get(url.as_str()).await.context("Fetching remote URL")?;
    ensure!(response.status().is_success(), "Remote URL did not reply with a successful code: {:?}", response.status());
    let text = response.text().await.context("Recovering the text part of the remote URL")?;

    // And parse it
    tracing::info!("Got response text: {:?}", text);
    Ok(icalendar::Calendar::new())
}

#[rocket::get("/<path>")]
async fn do_the_thing(path: &str, cfg: &rocket::State<Config>) -> Result<String, status::Custom<String>> {
    let remote_url = cfg.calendars.get(path)
        .ok_or_else(|| status::Custom(Status::NotFound, format!("Path {} is not configured", path)))?;

    let remote_ics = parse_remote_ics(&remote_url).await
        .map_err(|e| {
            warn!("Error parsing remote ICS: {:?}", e);
            status::Custom(Status::InternalServerError, format!("Error parsing remote ICS, see the logs for details"))
        })?;

    Ok("foo".to_string())
}

#[rocket::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opt::from_args();
    let config: Config = toml::from_str(&std::fs::read_to_string(&opts.config_file)?)?;

    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(tracing::Level::INFO)
            .finish()
    ).context("Setting tracing global subscriber")?;

    let rocket_config = rocket::Config {
        port: opts.port,
        address: opts.address,
        ..rocket::Config::default()
    };
    rocket::custom(&rocket_config)
        .manage(config)
        .mount("/", rocket::routes![do_the_thing])
        .launch()
        .await
        .context("Running rocket")
}
