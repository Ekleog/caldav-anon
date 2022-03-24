use anyhow::{ensure, Context};
use ical::parser::ical::component::IcalCalendar;
use rocket::{http::Status, response::status};
use tracing::warn;

pub async fn parse_remote_ics(url: &url::Url) -> anyhow::Result<IcalCalendar> {
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

pub fn build_property(
    name: &str,
    params: &Option<Vec<(String, Vec<String>)>>,
    value: &Option<String>,
) -> String {
    let mut res = name.to_string();
    if let Some(params) = params {
        for p in params {
            res = res + ";" + &p.0 + "=\"" + &p.1[0];
            for v in &p.1[1..] {
                res = res + "\",\"" + v;
            }
            res += "\"";
        }
    }
    res += ":\"";
    if let Some(value) = value {
        res += value;
    }
    res += "\"\n";
    res
}

pub async fn do_the_thing(
    path: &str,
    remote_url: Option<&url::Url>,
    generate_ics: impl FnOnce(IcalCalendar) -> anyhow::Result<String>,
) -> Result<String, status::Custom<String>> {
    let remote_url = remote_url.ok_or_else(|| {
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
    tracing::debug!("Got remote ICS {:?}", remote_ics);

    let generated_ics = generate_ics(remote_ics).map_err(|e| {
        warn!("Error generating scrubbed-out ICS from remote ICS: {:?}", e);
        status::Custom(
            Status::InternalServerError,
            format!("Error generating local ICS, see the logs for details\n"),
        )
    })?;
    tracing::debug!("Generated local ICS {:?}", generated_ics);

    Ok(generated_ics)
}
