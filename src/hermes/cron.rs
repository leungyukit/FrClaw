//! 5-field cron 表达式解析器。
//!
//! 格式：`分 时 日 月 周`
//! 字段语法：
//! - `*` —— 任意
//! - `5` —— 精确
//! - `1,3,5` —— 列表
//! - `1-5` —— 范围
//! - `*/5` —— 步长（从 0 开始）
//! - `1-10/2` —— 范围 + 步长
//!
//! 不支持秒级；不支持年。

use anyhow::{anyhow, Result};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CronField {
    Minute,   // 0-59
    Hour,     // 0-23
    Dom,      // 1-31
    Month,    // 1-12
    Dow,      // 0-6 (Sun=0)
}

impl CronField {
    fn range(&self) -> (i32, i32) {
        match self {
            CronField::Minute => (0, 59),
            CronField::Hour => (0, 23),
            CronField::Dom => (1, 31),
            CronField::Month => (1, 12),
            CronField::Dow => (0, 6),
        }
    }
}

/// 一个 cron 字段的合法值集合（Vec 排序）。
#[derive(Debug, Clone)]
struct FieldSpec(Vec<i32>);

impl FieldSpec {
    fn parse(field: CronField, raw: &str) -> Result<Self> {
        let (lo, hi) = field.range();
        let mut values = Vec::new();
        for part in raw.split(',') {
            let part = part.trim();
            if part.is_empty() {
                return Err(anyhow!("cron: 空字段"));
            }
            // step: `*/5` 或 `1-10/2`
            let (range_part, step) = if let Some((r, s)) = part.split_once('/') {
                let step: i32 = s.parse().map_err(|_| anyhow!("cron: 无效 step `{s}`"))?;
                if step <= 0 {
                    return Err(anyhow!("cron: step 必须 > 0"));
                }
                (r, step)
            } else {
                (part, 1)
            };
            let (start, end) = if range_part == "*" {
                (lo, hi)
            } else if let Some((a, b)) = range_part.split_once('-') {
                let a: i32 = a.parse().map_err(|_| anyhow!("cron: 无效数字 `{a}`"))?;
                let b: i32 = b.parse().map_err(|_| anyhow!("cron: 无效数字 `{b}`"))?;
                (a, b)
            } else {
                let n: i32 = range_part
                    .parse()
                    .map_err(|_| anyhow!("cron: 无效数字 `{range_part}`"))?;
                (n, n)
            };
            if start < lo || end > hi || start > end {
                return Err(anyhow!(
                    "cron: 字段 {:?} 范围 {}-{} 越界 {}-{}",
                    field,
                    start,
                    end,
                    lo,
                    hi
                ));
            }
            let mut v = start;
            while v <= end {
                values.push(v);
                v += step;
            }
        }
        values.sort();
        values.dedup();
        Ok(FieldSpec(values))
    }

    fn matches(&self, n: i32) -> bool {
        self.0.binary_search(&n).is_ok()
    }
}

#[derive(Debug, Clone)]
pub struct CronExpr {
    minute: FieldSpec,
    hour: FieldSpec,
    dom: FieldSpec,
    month: FieldSpec,
    dow: FieldSpec,
}

impl FromStr for CronExpr {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(anyhow!("cron: 需要 5 个字段（分 时 日 月 周），got {}", parts.len()));
        }
        Ok(Self {
            minute: FieldSpec::parse(CronField::Minute, parts[0])?,
            hour: FieldSpec::parse(CronField::Hour, parts[1])?,
            dom: FieldSpec::parse(CronField::Dom, parts[2])?,
            month: FieldSpec::parse(CronField::Month, parts[3])?,
            dow: FieldSpec::parse(CronField::Dow, parts[4])?,
        })
    }
}

impl CronExpr {
    pub fn new(s: &str) -> Result<Self> {
        s.parse()
    }

    /// 给定一个时间戳，算下一个匹配时刻（> ts，seconds 精度）。
    pub fn next_after(&self, ts: i64) -> Option<i64> {
        use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
        let dt: DateTime<Utc> = Utc.timestamp_opt(ts, 0).single()?;
        // 起点：下一分钟（用 NaiveDateTime 重置 seconds / nanos）
        let nd = dt.naive_utc();
        let next_min_naive = nd
            .with_second(0)?
            .with_nanosecond(0)?
            + chrono::Duration::minutes(1);
        let mut candidate: DateTime<Utc> = Utc.from_utc_datetime(&next_min_naive);
        // 最多循环 4 年（53 万分钟）防止死循环
        for _ in 0..(366 * 4 * 24 * 60) {
            if !self.month.matches(candidate.month() as i32) {
                let next = next_month_start(candidate);
                candidate = next;
                continue;
            }
            if !self.dom.matches(candidate.day() as i32)
                || !self.dow.matches(candidate.weekday().num_days_from_sunday() as i32)
            {
                let nd = candidate.naive_utc().date().succ_opt()?.and_hms_opt(0, 0, 0)?;
                candidate = Utc.from_utc_datetime(&nd);
                continue;
            }
            if !self.hour.matches(candidate.hour() as i32) {
                let nd = candidate.naive_utc().date().and_hms_opt(candidate.hour() + 1, 0, 0)?;
                candidate = Utc.from_utc_datetime(&nd);
                continue;
            }
            if !self.minute.matches(candidate.minute() as i32) {
                let nd = candidate.naive_utc() + chrono::Duration::minutes(1);
                candidate = Utc.from_utc_datetime(&nd);
                continue;
            }
            return Some(candidate.timestamp());
        }
        None
    }
}

fn next_month_start(dt: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
    use chrono::{Datelike, TimeZone, Utc};
    let mut y = dt.year();
    let mut m = dt.month() + 1;
    if m > 12 {
        m = 1;
        y += 1;
    }
    Utc.from_utc_datetime(
        &chrono::NaiveDate::from_ymd_opt(y, m, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn parse_basic() {
        let c = CronExpr::new("*/5 * * * *").unwrap();
        assert!(c.minute.matches(0));
        assert!(c.minute.matches(5));
        assert!(c.minute.matches(10));
        assert!(!c.minute.matches(3));
    }

    #[test]
    fn parse_list() {
        let c = CronExpr::new("0,30 * * * *").unwrap();
        assert!(c.minute.matches(0));
        assert!(c.minute.matches(30));
        assert!(!c.minute.matches(15));
    }

    #[test]
    fn parse_range() {
        // 0 9-17 * * * —— 第 2 字段（hour）范围 9-17
        let c = CronExpr::new("0 9-17 * * *").unwrap();
        assert!(c.hour.matches(9));
        assert!(c.hour.matches(17));
        assert!(!c.hour.matches(8));
        assert!(!c.hour.matches(18));
    }

    #[test]
    fn parse_invalid() {
        assert!(CronExpr::new("60 * * * *").is_err()); // min 越界
        assert!(CronExpr::new("* 24 * * *").is_err()); // hour 越界
        assert!(CronExpr::new("a b c d e").is_err()); // 5 段但无效
        assert!(CronExpr::new("* * * *").is_err()); // 4 段
    }

    #[test]
    fn next_after_basic() {
        let c = CronExpr::new("0 * * * *").unwrap();
        // 任意时间后下一个整点
        let ts = 1_700_000_000i64; // 2023-11-14 22:13:20 UTC
        let next = c.next_after(ts).unwrap();
        // 应是 23:00:00
        use chrono::TimeZone;
        let dt = chrono::Utc.timestamp_opt(next, 0).unwrap();
        assert_eq!(dt.hour(), 23);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn next_after_5min() {
        let c = CronExpr::new("*/5 * * * *").unwrap();
        let ts = 1_700_000_000i64; // 22:13:20
        let next = c.next_after(ts).unwrap();
        use chrono::TimeZone;
        let dt = chrono::Utc.timestamp_opt(next, 0).unwrap();
        // 下一个能被 5 整除的分钟
        assert_eq!(dt.minute() % 5, 0);
        // 必须是 15 或 20... 14:20:00, 14:25:00...
        assert!(dt.minute() == 15 || dt.minute() == 20 || dt.minute() > 20);
    }

    #[test]
    fn next_after_every_minute() {
        let c = CronExpr::new("* * * * *").unwrap();
        // 1_700_000_000 = 2023-11-14 22:13:20 UTC
        // 下一分钟 = 22:14:00 = +40 秒
        let ts = 1_700_000_000i64;
        let next = c.next_after(ts).unwrap();
        assert_eq!(next, ts + 40);
    }
}
