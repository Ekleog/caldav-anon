# ICS-Tools

## ICS-anon

ICS-anon is a proxy server, that re-serves an ICS calendar but anonymizes the detailed contents.

It is designed to be used to make it possible to publicize a private calendar without leaking more information than strictly required.

It can for instance be used to sync a Nextcloud Calendar to a Google Calendar as “busy” slots without leaking more information than strictly required.

### Usage

You can run ICS-anon with `cargo run --bin ics-anon -- -c <config-file>`. See `cargo run --bin ics-anon -- -h` for more command line flags.

The configuration file looks like this:
```toml
[config]
name = "The censored calendar's name"
message = "busy" # The message to use as summary of the censored events
seed = "Some hard-to-guess seed to protect the UIDs"
ignore_unknown_properties = false # Setting this to true can be useful when using a not-yet-supported ICS feed

[calendars]
path-1 = "https://remote-1/foo"
path-2 = "http://remote-2/bar?ics"
```

With this conifguration, `http://localhost:8000/path-1` will be an anonymized version of the ICS feed at `https://remote-1/foo`, and `http://localhost:8000/path-2` will be an anonymized version of the ICS feed at `http://remote-2/bar?ics`.

## ICS-ignore

ICS-ignore is a proxy server, that re-serves an ICS calendar but filters out events with defined contents.

It is designed to make it possible to avoid duplicates when synchronizing both a personal calendar and a work calendar that has the events anonymized by ICS-anon setup.

Such a setup would look like:
- Have a personal ICS feed
- Setup ICS-anon to anonymize this personal ICS feed (thereafter personal-anon)
- Import personal-anon into the work ICS feed
- Setup ICS-ignore to ignore the `busy` events from the work ICS feed (thereafter filtered-work)
- Have personal calendar applications fetch events from personal and filtered-work, thus avoiding duplicates

### Usage

You can run ICS-ignore with `cargo run --bin ics-ignore -- -c <config-file>`. See `cargo run --bin ics-ignore -- -h` for more command line flags.

The configuration file looks like this:
```toml
[config]
ignore_if_summary_is = "busy" # The summary of events to ignore (see also ics-anon's config.message)

[calendars]
path-1 = "https://remote-1/foo"
path-2 = "http://remote-2/bar?ics"
```

With this conifguration, `http://localhost:8000/path-1` will be a filtered version of the ICS feed at `https://remote-1/foo`, and `http://localhost:8000/path-2` will be a filtered version of the ICS feed at `http://remote-2/bar?ics`.

## See also

Other related projects:
- https://github.com/derekantrican/GAS-ICS-Sync (Force Google to refresh ICS calendars more often)
- https://github.com/utdemir/gcal-i-am-busy (Sync one Google Calendar with another one, marking the contents as only “busy”)

## Release Notes

### v0.1.0

- Basic functionality, enough to import a Nextcloud Calendar into a Google Calendar without leaking the event contents data
