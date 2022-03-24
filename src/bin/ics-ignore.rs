use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::{ensure, Context};
use ical::parser::ical::component::{IcalCalendar, IcalEvent, IcalTimeZone};
use rocket::response::status;
use structopt::StructOpt;

use ics_tools::*;

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
struct Cfg {
    /// The message to use as a summary in the generated events
    ignore_if_summary_is: String,
}

#[derive(serde::Deserialize)]
struct Config {
    config: Cfg,
    calendars: HashMap<String, url::Url>,
}

fn handle_calendar_properties(
    prop: &[ical::property::Property],
    _cfg: &Cfg,
    res: &mut String,
) -> anyhow::Result<()> {
    tracing::debug!("Property list: {:?}", prop);
    for p in prop {
        *res += &build_property(&p.name, &p.params, &p.value);
    }
    Ok(())
}

fn handle_timezones(tzs: &[IcalTimeZone], _cfg: &Cfg, res: &mut String) -> anyhow::Result<()> {
    for tz in tzs {
        *res += "BEGIN:VTIMEZONE\n";
        for p in &tz.properties {
            *res += &build_property(&p.name, &p.params, &p.value);
        }
        for transition in &tz.transitions {
            // TODO: ical doesn't expose whether it's BEGIN:DAYLIGHT or BEGIN:STANDARD
            // It probably doesn't matter anyway? I don't think the spec asks for any differential treatment at least
            *res += "BEGIN:STANDARD\n";
            for p in &transition.properties {
                *res += &build_property(&p.name, &p.params, &p.value);
            }
            *res += "END:STANDARD\n";
        }
        *res += "END:VTIMEZONE\n";
    }
    Ok(())
}

fn handle_events(evts: &[IcalEvent], cfg: &Cfg, res: &mut String) -> anyhow::Result<()> {
    'next_evt: for e in evts {
        // Skip event if it has an ignored summary
        for p in &e.properties {
            if p.name == "SUMMARY" && p.value.as_ref() == Some(&cfg.ignore_if_summary_is) {
                continue 'next_evt;
            }
        }
        // Otherwise, output event
        *res += "BEGIN:VEVENT\n";
        for p in &e.properties {
            *res += &build_property(&p.name, &p.params, &p.value);
        }
        *res += "END:VEVENT\n";
    }
    Ok(())
}

fn generate_ics(cal: IcalCalendar, cfg: &Cfg) -> anyhow::Result<String> {
    let mut res = "BEGIN:VCALENDAR\n".to_owned();

    handle_calendar_properties(&cal.properties, cfg, &mut res)
        .context("Handling the calendar properties")?;
    handle_timezones(&cal.timezones, cfg, &mut res).context("Handling the calendar timezones")?;
    handle_events(&cal.events, cfg, &mut res).context("Handling the calendar events")?;
    ensure!(
        cal.alarms.is_empty(),
        "Parsed calendar had alarms, this is not implemented yet, please open an issue"
    );
    ensure!(
        cal.todos.is_empty(),
        "Parsed calendar had todos, this is not implemented yet, please open an issue"
    );
    ensure!(
        cal.journals.is_empty(),
        "Parsed calendar had journals, this is not implemented yet, please open an issue"
    );
    ensure!(
        cal.free_busys.is_empty(),
        "Parsed calendar had free_busys, this is not implemented yet, please open an issue"
    );

    res += "END:VCALENDAR\n";

    Ok(res)
}

#[rocket::get("/<path>")]
async fn do_the_thing(
    path: &str,
    cfg: &rocket::State<Config>,
) -> Result<String, status::Custom<String>> {
    ics_tools::do_the_thing(
        path,
        cfg.calendars.get(path),
        |remote_ics| generate_ics(remote_ics, &cfg.config),
    ).await
}

#[rocket::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opt::from_args();
    let config: Config = toml::from_str(&std::fs::read_to_string(&opts.config_file)?)?;

    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(tracing::Level::INFO)
            .finish(),
    )
    .context("Setting tracing global subscriber")?;

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
