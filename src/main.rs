use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::Context;
use rocket::response::status::NotFound;
use structopt::StructOpt;

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

#[rocket::get("/<path>")]
fn do_the_thing(path: &str, cfg: &rocket::State<Config>) -> Result<String, NotFound<String>> {
    let remote_url = cfg.calendars.get(path)
        .ok_or_else(|| NotFound(format!("Path {} is not configured", path)))?;
    Ok(remote_url.to_string())
}

#[rocket::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opt::from_args();
    let config: Config = toml::from_str(&std::fs::read_to_string(&opts.config_file)?)?;

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
