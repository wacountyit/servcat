use crate::TargetUnit;

/// Formats a catalog item's target fulfillment time for display, e.g.
/// `"3 business days"`, `"30 minutes"`, `"1 hour"`. This is a *target*, not
/// a guarantee -- callers (catalog.html, the item detail page) should
/// label it that way (e.g. "Typical turnaround: ...") rather than implying
/// an SLA.
///
/// An exact multiple of 24 hours collapses to calendar days (24 -> "1
/// day", 48 -> "2 days") since that's an unambiguous conversion -- 24
/// hours *is* one calendar day, full stop. Anything else (23 hours, a
/// business-day count, etc.) is shown exactly as stored: converting "3
/// business days" into an hour count, for instance, would silently invent
/// a workday length nobody configured.
pub fn format_target(value: i32, unit: TargetUnit) -> String {
    if unit == TargetUnit::Hours && value != 0 && value % 24 == 0 {
        let days = value / 24;
        return format!("{days} {}", TargetUnit::CalendarDays.noun(days));
    }
    format!("{value} {}", unit.noun(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pluralizes_each_unit() {
        assert_eq!(format_target(1, TargetUnit::Minutes), "1 minute");
        assert_eq!(format_target(30, TargetUnit::Minutes), "30 minutes");
        assert_eq!(format_target(1, TargetUnit::Hours), "1 hour");
        assert_eq!(format_target(1, TargetUnit::BusinessDays), "1 business day");
        assert_eq!(
            format_target(3, TargetUnit::BusinessDays),
            "3 business days"
        );
        assert_eq!(format_target(1, TargetUnit::CalendarDays), "1 day");
        assert_eq!(format_target(5, TargetUnit::CalendarDays), "5 days");
    }

    #[test]
    fn treats_zero_as_plural() {
        // Not a value this app should ever actually store, but the noun
        // logic shouldn't panic or produce nonsense ("0 minute") on it.
        assert_eq!(format_target(0, TargetUnit::Minutes), "0 minutes");
    }

    #[test]
    fn collapses_exact_24_hour_multiples_to_calendar_days() {
        assert_eq!(format_target(24, TargetUnit::Hours), "1 day");
        assert_eq!(format_target(48, TargetUnit::Hours), "2 days");
        assert_eq!(format_target(72, TargetUnit::Hours), "3 days");
    }

    #[test]
    fn leaves_non_24_multiples_and_other_units_as_stored() {
        // 23 and 25 hours are *not* an unambiguous whole number of days.
        assert_eq!(format_target(23, TargetUnit::Hours), "23 hours");
        assert_eq!(format_target(25, TargetUnit::Hours), "25 hours");
        // Business days must never be silently reinterpreted as hours/
        // calendar days -- a "business day" length isn't a fixed constant.
        assert_eq!(
            format_target(24, TargetUnit::BusinessDays),
            "24 business days"
        );
    }
}
