use chrono::{
    DateTime, NaiveDateTime, TimeZone, Utc,
};

pub fn round_up_datetime(time_to_round: DateTime<Utc>, period_in_secs: i64) -> DateTime<Utc> {
    let since_unix_epoch = time_to_round.timestamp();

    let remainder = since_unix_epoch % period_in_secs;
    if remainder == 0 {
        return time_to_round;
    }

    let round_timestamp =
        NaiveDateTime::from_timestamp_opt(since_unix_epoch + period_in_secs - remainder, 0)
            .unwrap();

    DateTime::<Utc>::from_utc(round_timestamp, Utc)
}

pub fn until_event(period_in_secs: u64) -> std::time::Duration {
    let now = Utc::now();
    round_up_datetime(now, period_in_secs as i64)
        .signed_duration_since(now)
        .to_std()
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::{assert_eq, assert_ne};
    use rstest::*;
    use chrono::NaiveDate;

    #[rstest]
    #[case::next_window((2022, 6, 20, 20, 10, 37), 60, (2022, 6, 20, 20, 11, 00))]
    #[case::current_window((2022, 6, 20, 20, 11, 00), 60, (2022, 6, 20, 20, 11, 00))]
    #[case::next_window_multiple_minutes((2022, 6, 20, 20, 10, 17), 600, (2022, 6, 20, 20, 20, 00))]
    #[case::next_window_multiple_hours((2022, 6, 20, 21, 10, 17), 4 * 60 * 60, (2022, 6, 21, 00, 00, 00))]
    #[case::next_window_multiple_hours((2022, 6, 20, 21, 10, 17), 3 * 60 * 60, (2022, 6, 21, 00, 00, 00))]
    #[case::next_window_multiple_days((2022, 6, 20, 13, 10, 17), 48 * 60 * 60, (2022, 6, 21, 00, 00, 00))] // figure out whether it is suffiecient
    fn test_rounding_up_minutes(
        #[case] input: (i32, u32, u32, u32, u32, u32),
        #[case] period_in_secs: i64,
        #[case] expected: (i32, u32, u32, u32, u32, u32),
    ) {
        // given
        let time_to_round: DateTime<Utc> = (*Utc::now().offset())
            .from_utc_datetime(
                &NaiveDate::from_ymd_opt(input.0, input.1, input.2)
                    .unwrap()
                    .and_hms_opt(input.3, input.4, input.5)
                    .unwrap(),
            )
            .into();

        let expected: DateTime<Utc> = (*Utc::now().offset())
            .from_utc_datetime(
                &NaiveDate::from_ymd_opt(expected.0, expected.1, expected.2)
                    .unwrap()
                    .and_hms_opt(expected.3, expected.4, expected.5)
                    .unwrap(),
            )
            .into();

        // when
        let result = round_up_datetime(time_to_round, period_in_secs);

        // then
        assert_eq!(result, expected)
    }
}
