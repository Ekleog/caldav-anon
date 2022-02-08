use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::{ensure, Context};
use hmac::Mac;
use ical::parser::ical::component::{IcalCalendar, IcalEvent, IcalTimeZone};
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
struct Cfg {
    /// The message to use as a summary in the generated events
    message: String,

    /// The seed to use for hashing the UIDs of calendar events. Should ideally be unguessable
    seed: String,

    /// Whether to ignore all unknown properties
    #[serde(default)] // bool::default() is `false`
    ignore_unknown_properties: bool,
}

#[derive(serde::Deserialize)]
struct Config {
    config: Cfg,
    calendars: HashMap<String, url::Url>,
}

async fn parse_remote_ics(url: &url::Url) -> anyhow::Result<IcalCalendar> {
    // Fetch the remote ICS file
    let response = reqwest::get(url.as_str())
        .await
        .with_context(|| format!("Fetching remote URL {}", url))?;
    ensure!(
        response.status().is_success(),
        "Remote URL {} did not reply with a successful code: {:?}",
        url,
        response.status()
    );
    let text = response
        .text()
        .await
        .with_context(|| format!("Recovering the text part of the remote URL {}", url))?;

    // And parse it
    let calendars = ical::IcalParser::new(text.as_bytes()).collect::<Vec<_>>();
    ensure!(calendars.len() == 1, "Remote URL {} had multiple calendars, this is not supported yet, please open an issue if you have a use case for it", url);
    let calendar = calendars.into_iter().next().unwrap(); // see ensure! juste above

    calendar.with_context(|| format!("Failed to parse the calendar for remote URL {}", url))
}

fn build_property(
    name: &str,
    params: &Option<Vec<(String, Vec<String>)>>,
    value: &Option<String>,
) -> String {
    let mut res = name.to_string();
    if let Some(params) = params {
        for p in params {
            res = res + ";" + &p.0 + "=" + &p.1[0];
            for v in &p.1[1..] {
                res = res + "," + v;
            }
        }
    }
    res += ":";
    if let Some(value) = value {
        res += value;
    }
    res += "\n";
    res
}

macro_rules! unknown_property {
    ($type:expr, $cfg:expr, $propname:expr) => {
        if $cfg.ignore_unknown_properties {
            tracing::warn!("Found unknown {} property {}, ignoring", $type, $propname);
        } else {
            anyhow::bail!("Found unknown {} property {}, please open an issue and consider using `ignore_unknown_properties`", $type, $propname);
        }
    }
}

fn handle_calendar_properties(
    prop: &[ical::property::Property],
    cfg: &Cfg,
    res: &mut String,
) -> anyhow::Result<()> {
    tracing::info!("Property list: {:?}", prop);
    for p in prop {
        match &p.name as &str {
            // Proxy all important properties
            "CALSCALE" => *res += &build_property("CALSCALE", &p.params, &p.value),
            // Censor all non-required properties
            "METHOD" => (),
            "PRODID" => (),
            "REFRESH-INTERVAL" => (),
            "VERSION" if p.value.as_ref().map(|v| v as &str) == Some("2.0") => (),
            _ if p.name.starts_with("X-") => (),
            // And either warn or bail on unknown properties
            _ => unknown_property!("calendar", cfg, p.name),
        }
    }
    Ok(())
}

fn handle_timezones(tzs: &[IcalTimeZone], cfg: &Cfg, res: &mut String) -> anyhow::Result<()> {
    for tz in tzs {
        *res += "BEGIN:VTIMEZONE\n";
        for p in &tz.properties {
            match &p.name as &str {
                // Proxy all important properties
                "TZID" => {
                    *res += &build_property(&p.name, &p.params, &p.value);
                }
                // And either warn or bail on the other properties
                _ => unknown_property!("timezone", cfg, p.name),
            }
        }
        for transition in &tz.transitions {
            // TODO: ical doesn't expose whether it's BEGIN:DAYLIGHT or BEGIN:STANDARD
            // It probably doesn't matter anyway? I don't think the spec asks for any differential treatment at least
            *res += "BEGIN:STANDARD\n";
            for p in &transition.properties {
                match &p.name as &str {
                    // Proxy all important properties
                    "DTSTART" | "RRULE" | "TZNAME" | "TZOFFSETFROM" | "TZOFFSETTO" => {
                        *res += &build_property(&p.name, &p.params, &p.value);
                    }
                    // And either warn or bail on unknown properties
                    _ => unknown_property!("timezone transition", cfg, p.name),
                }
            }
            *res += "END:STANDARD\n";
        }
        *res += "END:VTIMEZONE\n";
    }
    Ok(())
}

fn handle_events(evts: &[IcalEvent], cfg: &Cfg, res: &mut String) -> anyhow::Result<()> {
    for e in evts {
        *res += &format!(
            "BEGIN:VEVENT\n\
             SUMMARY:{}\n\
             DTSTAMP:20200101T000001Z\n",
            cfg.message
        );
        // Ignore all alarms, as we only care about busy-ness
        // Go through the interesting properties
        for p in &e.properties {
            match &p.name as &str {
                // Proxy all important properties
                "DTSTART" | "DTEND" | "EXDATE" | "EXRULE" | "RDATE" | "RRULE" | "SEQUENCE"
                | "STATUS" => {
                    *res += &build_property(&p.name, &p.params, &p.value);
                }
                "UID" => {
                    if let Some(value) = &p.value {
                        let mut hasher =
                            hmac::Hmac::<sha2::Sha256>::new_from_slice(cfg.seed.as_bytes())
                                .context("Initializing hasher with seed")?;
                        hasher.update(value.as_bytes());
                        let hash = hasher.finalize().into_bytes();
                        *res += &format!("UID:{}\n", hex::encode(hash));
                    }
                }
                // Censor all non-required properties
                "CREATED" => (),
                "DTSTAMP" => (),
                "DESCRIPTION" => (),
                "LAST-MODIFIED" => (),
                "LOCATION" => (),
                "SUMMARY" => (),
                "URL" => (),
                // And either warn or bail on the other properties
                _ => unknown_property!("event", cfg, p.name),
            }
        }
        *res += "END:VEVENT\n";
    }
    Ok(())
}

fn generate_ics(cal: IcalCalendar, cfg: &Cfg) -> anyhow::Result<String> {
    let mut res = "BEGIN:VCALENDAR\n\
                   VERSION:2.0\n\
                   PRODID:CALDAV-ANON\n"
        .to_string();

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
    let remote_url = cfg.calendars.get(path).ok_or_else(|| {
        status::Custom(
            Status::NotFound,
            format!("Path {} is not configured\n", path),
        )
    })?;

    let remote_ics = parse_remote_ics(&remote_url).await.map_err(|e| {
        warn!("Error parsing remote ICS: {:?}", e);
        status::Custom(
            Status::InternalServerError,
            format!("Error parsing remote ICS, see the logs for details\n"),
        )
    })?;
    tracing::info!("Got remote ICS {:?}", remote_ics);

    let generated_ics = generate_ics(remote_ics, &cfg.config).map_err(|e| {
        warn!("Error generating scrubbed-out ICS from remote ICS: {:?}", e);
        status::Custom(
            Status::InternalServerError,
            format!("Error generating local ICS, see the logs for details\n"),
        )
    })?;
    tracing::info!("Generated local ICS {:?}", generated_ics);

    Ok(generated_ics)
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
