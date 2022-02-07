use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::{ensure, Context};
use ical::parser::ical::component::{IcalCalendar, IcalEvent};
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

async fn parse_remote_ics(url: &url::Url) -> anyhow::Result<IcalCalendar> {
    // Fetch the remote ICS file
    let response = reqwest::get(url.as_str()).await.with_context(|| format!("Fetching remote URL {}", url))?;
    ensure!(response.status().is_success(), "Remote URL {} did not reply with a successful code: {:?}", url, response.status());
    let text = response.text().await.with_context(|| format!("Recovering the text part of the remote URL {}", url))?;

    // And parse it
    let calendars = ical::IcalParser::new(text.as_bytes()).collect::<Vec<_>>();
    ensure!(calendars.len() == 1, "Remote URL {} had multiple calendars, this is not supported yet, please open an issue if you have a use case for it", url);
    let calendar = calendars.into_iter().next().unwrap(); // see ensure! juste above

    calendar.with_context(|| format!("Failed to parse the calendar for remote URL {}", url))
}

fn handle_calendar_properties(prop: &[ical::property::Property], _res: &mut icalendar::Calendar) -> anyhow::Result<()> {
    tracing::info!("Property list: {:?}", prop);
    for p in prop {
        match &p.name as &str {
            // Censor all non-required properties
            "CALSCALE" => (),
            "METHOD" => (),
            "PRODID" => (),
            "REFRESH-INTERVAL" => (),
            "VERSION" if p.value.as_ref().map(|v| v as &str) == Some("2.0") => (),
            _ if p.name.starts_with("X-") => (),
            _ => anyhow::bail!("Found unknown property, please open an issue: {}", p.name),
        }
    }
    Ok(())
}

fn handle_events(evts: &[IcalEvent], res: &mut icalendar::Calendar) -> anyhow::Result<()> {
    for e in evts {
        ensure!(e.alarms.is_empty(), "Parsed calendar event has alarms, this is not implemented yet, please open an issue");
        for p in &e.properties {
            todo!()
        }
    }
    Ok(())
}

fn generate_calendar(cal: IcalCalendar) -> anyhow::Result<icalendar::Calendar> {
    let mut res = icalendar::Calendar::new();

    handle_calendar_properties(&cal.properties, &mut res).context("Handling the calendar properties")?;
    handle_events(&cal.events, &mut res).context("Handling the calendar events")?;
    ensure!(cal.alarms.is_empty(), "Parsed calendar had alarms, this is not implemented yet, please open an issue");
    ensure!(cal.todos.is_empty(), "Parsed calendar had todos, this is not implemented yet, please open an issue");
    ensure!(cal.journals.is_empty(), "Parsed calendar had journals, this is not implemented yet, please open an issue");
    ensure!(cal.free_busys.is_empty(), "Parsed calendar had free_busys, this is not implemented yet, please open an issue");
    ensure!(cal.timezones.is_empty(), "Parsed calendar had timezones, this is not implemented yet, please open an issue");

    Ok(res)
}

#[rocket::get("/<path>")]
async fn do_the_thing(path: &str, cfg: &rocket::State<Config>) -> Result<String, status::Custom<String>> {
    let remote_url = cfg.calendars.get(path)
        .ok_or_else(|| status::Custom(Status::NotFound, format!("Path {} is not configured\n", path)))?;

    let remote_ics = parse_remote_ics(&remote_url).await
        .map_err(|e| {
            warn!("Error parsing remote ICS: {:?}", e);
            status::Custom(Status::InternalServerError, format!("Error parsing remote ICS, see the logs for details\n"))
        })?;
    tracing::info!("Got remote ICS {:?}", remote_ics);

    let generated_calendar = generate_calendar(remote_ics)
        .map_err(|e| {
            warn!("Error generating scrubbed-out ICS from remote ICS: {:?}", e);
            status::Custom(Status::InternalServerError, format!("Error generating local ICS, see the logs for details\n"))
        })?;
    tracing::info!("Generated local ICS {:?}", generated_calendar);

    let generated_ics = (&generated_calendar).try_into()
        .map_err(|e| {
            warn!("Error lowering scrubbed-out ICS to string: {:?}", e);
            status::Custom(Status::InternalServerError, format!("Error lowering local ICS to string, see the logs for details\n"))
        })?;

    Ok(generated_ics)
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
