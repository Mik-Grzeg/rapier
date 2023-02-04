use chrono::{DateTime, Utc};
use reqwest::Response;

use std::{collections::BTreeMap, iter::Sum};

extern crate pretty_env_logger;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThroughputMetric {
    xx2: u32,
    xx3: u32,
    xx4: u32,
    xx5: u32,
    other: u32,
}

impl ThroughputMetric {
    pub fn new((xx2, xx3, xx4, xx5, other): (u32, u32, u32, u32, u32)) -> Self {
        Self {
            xx2,
            xx3,
            xx4,
            xx5,
            other,
        }
    }

    pub fn flush_current_metric(&mut self) {
        self.xx2 = 0;
        self.xx3 = 0;
        self.xx4 = 0;
        self.xx5 = 0;
        self.other = 0;
    }

    pub fn record_response(&mut self, response: Option<&Response>) {
        match response {
            Some(response) => {
                let response = response.status();

                if response.is_success() {
                    self.increment_xx2()
                } else if response.is_redirection() {
                    self.increment_xx3()
                } else if response.is_client_error() {
                    self.increment_xx4()
                } else if response.is_server_error() {
                    self.increment_xx5()
                } else {
                    self.increment_other()
                }
            }
            None => self.increment_other(),
        }
    }

    pub fn increment_xx2(&mut self) {
        self.xx2 += 1;
    }

    pub fn increment_xx3(&mut self) {
        self.xx3 += 1;
    }

    pub fn increment_xx4(&mut self) {
        self.xx4 += 1;
    }

    pub fn increment_xx5(&mut self) {
        self.xx5 += 1;
    }

    pub fn increment_other(&mut self) {
        self.other += 1;
    }
}

impl std::ops::Add for ThroughputMetric {
    type Output = ThroughputMetric;
    fn add(self, rhs: Self) -> Self::Output {
        Self::Output {
            xx2: self.xx2 + rhs.xx2,
            xx3: self.xx3 + rhs.xx3,
            xx4: self.xx4 + rhs.xx4,
            xx5: self.xx5 + rhs.xx5,
            other: self.other + rhs.other,
        }
    }
}

impl Sum<ThroughputMetric> for ThroughputMetric {
    fn sum<I: Iterator<Item = ThroughputMetric>>(iter: I) -> Self {
        iter.reduce(|acc, e| acc + e).unwrap()
    }
}

impl std::fmt::Display for ThroughputMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"=======================
|   2xx: {:width$}   |
|   3xx: {:width$}   |
|   4xx: {:width$}   |
|   5xx: {:width$}   |
| other: {:width$}   |
======================="#,
            self.xx2,
            self.xx3,
            self.xx4,
            self.xx5,
            self.other,
            width = 10
        )
    }
}

pub trait Reporter {
    fn add_record(&mut self, key: i64, value: ThroughputMetric);
    fn get_metrics_for_point_in_time(&self, t: &DateTime<Utc>) -> &ThroughputMetric;
    fn get_metrics_for_period(&self, t1: &DateTime<Utc>, t2: &DateTime<Utc>) -> ThroughputMetric;
}

#[derive(Debug, Default)]
pub struct MetricsStore {
    data: BTreeMap<i64, ThroughputMetric>,
}

impl Reporter for MetricsStore {
    fn add_record(&mut self, key: i64, value: ThroughputMetric) {
        self.data.insert(key, value);
    }

    fn get_metrics_for_point_in_time(&self, t: &DateTime<Utc>) -> &ThroughputMetric {
        let elem = self.data.get(&t.timestamp()).unwrap();
        info!("{}", *elem);
        elem
    }

    fn get_metrics_for_period(&self, t1: &DateTime<Utc>, t2: &DateTime<Utc>) -> ThroughputMetric {
        let bounds = t1.timestamp()..t2.timestamp();
        let (_, values): (Vec<&i64>, Vec<ThroughputMetric>) = self.data.range(bounds).unzip();
        let sum: ThroughputMetric = values.into_iter().sum();

        info!("{}", sum);
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate, TimeZone};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_adding_to_metrics_store() {
        // given
        let mut store = MetricsStore::default();

        let record = (*Utc::now().offset())
            .from_utc_datetime(
                &NaiveDate::from_ymd_opt(2022, 6, 20)
                    .unwrap()
                    .and_hms_opt(20, 10, 30)
                    .unwrap(),
            )
            .timestamp();
        let metric = ThroughputMetric {
            xx2: 10,
            xx3: 15,
            xx4: 20,
            xx5: 30,
            other: 0,
        };
        // when
        store.add_record(record, metric);

        //then
        assert_eq!(*store.data.get(&record).unwrap(), metric)
    }

    #[test]
    fn test_adding_and_getting_single_metric() {
        // given
        let mut store = MetricsStore::default();

        let record = (*Utc::now().offset()).from_utc_datetime(
            &NaiveDate::from_ymd_opt(2022, 6, 20)
                .unwrap()
                .and_hms_opt(20, 10, 30)
                .unwrap(),
        );
        let metric = ThroughputMetric::new((15, 20, 30, 0, 17));
        // when
        store.add_record(record.timestamp(), metric);
        let retrieved_metric = store.get_metrics_for_point_in_time(&record);

        //then
        assert_eq!(*retrieved_metric, metric)
    }

    #[test]
    fn test_adding_and_getting_aggregated_metrics_from_period() {
        // given
        let mut store = MetricsStore::default();

        let start = (*Utc::now().offset()).from_utc_datetime(
            &NaiveDate::from_ymd_opt(2022, 6, 20)
                .unwrap()
                .and_hms_opt(20, 10, 30)
                .unwrap(),
        );

        let period_in_secs = 15;
        let points_in_time: Vec<DateTime<Utc>> = (0..3)
            .map(|i| start + Duration::seconds(i) * period_in_secs)
            .collect();

        let metrics = vec![(23, 21, 0, 30, 1), (7, 0, 0, 1, 1), (20, 21, 5, 4, 1)];
        let recorded_metrics = metrics.into_iter().zip(points_in_time.clone());

        recorded_metrics.into_iter().for_each(|(metric, time)| {
            store.add_record(time.timestamp(), ThroughputMetric::new(metric))
        });
        let expected_aggregated_metric = ThroughputMetric::new((30, 21, 0, 31, 2));
        // when
        let retrieved_metric = store.get_metrics_for_period(
            &points_in_time.first().unwrap(),
            &points_in_time.last().unwrap(),
        );

        //then
        assert_eq!(retrieved_metric, expected_aggregated_metric)
    }

    #[test]
    fn test_to_string_metrics() {
        // given
        let metric = ThroughputMetric::new((15, 20, 30, 0, 170));

        let expected_str: &str = r#"=======================
|   2xx:         15   |
|   3xx:         20   |
|   4xx:         30   |
|   5xx:          0   |
| other:        170   |
======================="#;

        // when
        let result = metric.to_string();

        // then
        assert_eq!(result, expected_str);
    }
}
