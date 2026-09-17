//! Helpers for `org_settings.timezone` (an admin-configurable IANA name,
//! e.g. `"America/Chicago"`) -- used only to *render* timestamps in the web
//! UI; every stored timestamp remains UTC regardless of this setting.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

/// Every IANA timezone name `chrono-tz` knows about, for the admin settings
/// page's timezone `<select>`. Not sorted by `chrono-tz` itself, so callers
/// that want a stable/alphabetical order should sort this.
pub fn all_names() -> impl Iterator<Item = &'static str> {
    chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name())
}

pub fn is_valid(name: &str) -> bool {
    name.parse::<Tz>().is_ok()
}

/// Renders `dt` in `tz_name`, falling back to UTC if `tz_name` isn't a
/// recognized IANA name. That shouldn't happen in practice -- `is_valid`
/// gates every write to `org_settings.timezone` -- but display must never
/// panic over a bad stored value.
pub fn format_local(dt: &DateTime<Utc>, tz_name: &str) -> String {
    match tz_name.parse::<Tz>() {
        Ok(tz) => dt
            .with_timezone(&tz)
            .format("%Y-%m-%d %H:%M %Z")
            .to_string(),
        Err(_) => dt.format("%Y-%m-%d %H:%M UTC").to_string(),
    }
}
